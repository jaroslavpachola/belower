// Copyright (c) Facebook, Inc. and its affiliates.
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

use chrono::DateTime;
use chrono::Local;
use cursive::Cursive;
use cursive::event::Event;
use cursive::utils::markup::StyledString;
use cursive::view::Nameable;
use cursive::view::View;
use cursive::views::TextView;

use crate::ViewMode;
use crate::ViewState;
use crate::controllers::Controllers;
use crate::controllers::event_to_string;

fn get_spacing() -> &'static str {
    "     "
}

/// How a key is shown in the hints: "q", "space", "^n".
fn key_label(event: &Event) -> String {
    match event {
        Event::Char(' ') => "space".into(),
        Event::Char(c) => c.to_string(),
        Event::CtrlChar(c) => format!("^{c}"),
        other => event_to_string(other),
    }
}

/// Hints for the most useful keys in the current mode, using the keys they
/// are actually bound to (belowrc can remap them).
fn key_hints(view_state: &ViewState) -> String {
    let event_controllers = view_state.event_controllers.lock().unwrap();
    // The shortest label of the keys bound to `controller`, if any.
    let key = |controller: Controllers| {
        event_controllers
            .iter()
            .filter(|(_, c)| **c == controller)
            .map(|(event, _)| key_label(event))
            .min_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)))
    };

    let mut hints = vec![
        (key(Controllers::Help), "help"),
        (key(Controllers::Filter), "filter"),
        (key(Controllers::SortCol), "sort"),
    ];
    let jump = || match (key(Controllers::JForward), key(Controllers::JBackward)) {
        (Some(forward), Some(backward)) => Some(format!("{forward}/{backward}")),
        _ => None,
    };
    match view_state.mode {
        ViewMode::Live(_) => hints.push((key(Controllers::Pause), "pause")),
        ViewMode::Pause(_) => {
            hints.push((key(Controllers::Pause), "resume"));
            hints.push((jump(), "jump"));
        }
        ViewMode::Replay(_) => hints.push((jump(), "jump")),
    }
    hints.push((key(Controllers::Quit), "quit"));

    hints
        .into_iter()
        .filter_map(|(key, what)| Some(format!("{}:{what}", key?)))
        .collect::<Vec<_>>()
        .join("  ")
}

fn get_content(c: &mut Cursive) -> impl Into<StyledString> + use<> {
    let view_state = &c
        .user_data::<ViewState>()
        .expect("No data stored in Cursive object!");
    let datetime = DateTime::<Local>::from(view_state.timestamp);
    let mut header_str = StyledString::plain(format!(
        "{}{}",
        datetime.format("%m/%d/%Y %H:%M:%S UTC%:z"),
        get_spacing()
    ));

    header_str.append_plain("Elapsed: ");
    let elapsed_rendered = format!("{}s", view_state.time_elapsed.as_secs(),);
    let lowest = view_state.lowest_time_elapsed.as_secs();
    let this = view_state.time_elapsed.as_secs();
    // 1 second jitter happens pretty often due to integer rounding
    if lowest != 0 && this >= (lowest + 2) {
        header_str.append_styled(
            elapsed_rendered,
            cursive::theme::Color::Light(cursive::theme::BaseColor::Red),
        );
    } else {
        header_str.append_plain(elapsed_rendered);
    }

    header_str.append_plain(format!(
        "{}{}{}",
        get_spacing(),
        &view_state.system.lock().unwrap().hostname,
        get_spacing(),
    ));

    header_str.append_plain(crate::get_version_str());
    header_str.append_plain(get_spacing());
    header_str.append_plain(view_state.view_mode_str());

    if view_state.width_delta != 0 {
        header_str.append_plain(format!(
            "{}[Width: {:+}]",
            get_spacing(),
            view_state.width_delta
        ));
    }

    let hints = key_hints(view_state);
    if !hints.is_empty() {
        header_str.append_plain(get_spacing());
        header_str.append_styled(
            hints,
            cursive::theme::Color::Dark(cursive::theme::BaseColor::Cyan),
        );
    }

    header_str
}

pub fn refresh(c: &mut Cursive) {
    let content = get_content(c);
    let mut v = c
        .find_name::<TextView>("status_bar")
        .expect("No status_bar view found!");
    v.set_content(content);
}

pub fn new(c: &mut Cursive) -> impl View + use<> {
    // Cut the line off rather than wrapping it on narrow terminals.
    TextView::new(get_content(c))
        .no_wrap()
        .with_name("status_bar")
}
