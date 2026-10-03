// Copyright (c) 2026 Jaroslav Pachola
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! The view as the user left it: the screen, each screen's tab and sort
//! column, and the column width adjustment. Saved when the view exits and
//! restored when it starts, in ~/.local/state/belower/view.toml.

use std::collections::BTreeMap;
use std::path::PathBuf;

use cursive::Cursive;
use serde::Deserialize;
use serde::Serialize;

use crate::MainViewState;
use crate::ViewState;
use crate::cgroup_view::CgroupView;
use crate::process_view::ProcessView;
use crate::stats_view::StateCommon;
use crate::stats_view::StatsView;
use crate::stats_view::ViewBridge;
use crate::system_view::SystemView;
use crate::viewrc::DefaultFrontView;

#[derive(Default, Serialize, Deserialize, Debug, PartialEq)]
#[serde(default)]
pub struct RememberedView {
    pub screen: Option<DefaultFrontView>,
    pub width_delta: i32,
    /// Keyed by view name, e.g. "cgroup_view".
    pub screens: BTreeMap<String, ScreenState>,
}

#[derive(Default, Serialize, Deserialize, Debug, PartialEq)]
#[serde(default)]
pub struct ScreenState {
    pub tab: Option<String>,
    /// The field sorted by, as its field ID, e.g. "mem.total".
    pub sort: Option<String>,
    pub reverse: bool,
}

/// ~/.local/state/belower/view.toml, or under $XDG_STATE_HOME. The view is
/// per person, so this is used as root too.
fn path() -> Option<PathBuf> {
    let env = |var| std::env::var_os(var).filter(|v| !v.is_empty());
    let state_home = env("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| env("HOME").map(|home| PathBuf::from(home).join(".local/state")))?;
    Some(state_home.join("belower/view.toml"))
}

impl RememberedView {
    /// The remembered view, or the default if there is none or it is
    /// unreadable.
    pub fn load() -> Self {
        path()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| toml::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Save, ignoring failures: remembering the view is a convenience.
    pub fn save(&self) {
        let (Some(path), Ok(text)) = (path(), toml::to_string(self)) else {
            return;
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, text);
    }

    /// Capture the view's current state.
    pub fn capture(c: &mut Cursive) -> Self {
        let view_state = c.user_data::<ViewState>().expect("No user data set");
        let screen = match view_state.main_view_state {
            MainViewState::Cgroup => Some(DefaultFrontView::Cgroup),
            MainViewState::Process(_) => Some(DefaultFrontView::Process),
            MainViewState::System => Some(DefaultFrontView::System),
            #[cfg(fbcode_build)]
            MainViewState::Gpu => None,
        };
        let width_delta = view_state.width_delta;
        let mut screens = BTreeMap::new();
        capture_screen::<CgroupView>(c, &mut screens);
        capture_screen::<ProcessView>(c, &mut screens);
        capture_screen::<SystemView>(c, &mut screens);
        Self {
            screen,
            width_delta,
            screens,
        }
    }

    /// Apply the remembered tabs, sorting and width to the view. The screen
    /// is left to the caller, since belowrc's default_view takes precedence.
    pub fn restore(&self, c: &mut Cursive) {
        c.user_data::<ViewState>()
            .expect("No user data set")
            .width_delta = self.width_delta;
        restore_screen::<CgroupView>(c, &self.screens);
        restore_screen::<ProcessView>(c, &self.screens);
        restore_screen::<SystemView>(c, &self.screens);
    }
}

fn capture_screen<V: 'static + ViewBridge>(
    c: &mut Cursive,
    screens: &mut BTreeMap<String, ScreenState>,
) {
    let mut view = StatsView::<V>::get_view(c);
    let tab = view.get_tab_view().get_cur_selected().trim().to_string();
    let sort = view.state.lock().unwrap().get_sort_string();
    let reverse = view.reverse_sort;
    screens.insert(
        V::get_view_name().to_string(),
        ScreenState {
            tab: Some(tab),
            sort,
            reverse,
        },
    );
}

fn restore_screen<V: 'static + ViewBridge>(
    c: &mut Cursive,
    screens: &BTreeMap<String, ScreenState>,
) {
    let Some(screen) = screens.get(V::get_view_name()) else {
        return;
    };
    let mut view = StatsView::<V>::get_view(c);
    if let Some(tab) = &screen.tab {
        let found = {
            let mut tab_view = view.get_tab_view();
            match tab_view.tabs.iter().position(|t| t.trim() == tab) {
                Some(idx) => {
                    while tab_view.current_selected != idx {
                        tab_view.on_tab();
                    }
                    true
                }
                None => false,
            }
        };
        if found {
            view.update_title();
        }
    }
    if let Some(sort) = &screen.sort {
        let view = &mut *view;
        let mut state = view.state.lock().unwrap();
        // Sorting by a new field sorts descending; sorting by it again
        // reverses that.
        if state.set_sort_string(sort, &mut view.reverse_sort)
            && view.reverse_sort != screen.reverse
        {
            state.set_sort_string(sort, &mut view.reverse_sort);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_toml() {
        let view = RememberedView {
            screen: Some(DefaultFrontView::Process),
            width_delta: -3,
            screens: BTreeMap::from([(
                "cgroup_view".to_string(),
                ScreenState {
                    tab: Some("Mem".into()),
                    sort: Some("mem.total".into()),
                    reverse: false,
                },
            )]),
        };
        let text = toml::to_string(&view).unwrap();
        assert_eq!(toml::from_str::<RememberedView>(&text).unwrap(), view);
        // Missing fields fall back to defaults.
        assert_eq!(
            toml::from_str::<RememberedView>("").unwrap(),
            RememberedView::default()
        );
    }
}
