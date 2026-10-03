# Belower runtime config

`belower` will use `$HOME/.config/belower/belowerrc` toml file for customized configuration. Here's an example belowerrc file:

```toml
[dump.system]
any_name_will_work = ["datetime", "os_release"]
my_pattern = ["timestamp", "cpu.usage_pct"]

[cmd]
next_tab = 'b'
prev_tab = 'l'

[view]
collapse_cgroups = true
default_view = "process"
```

## dump.SUBCOMMAND

`belower` support saving customized dump pattern in the `[dump.{SUBCOMMAND}]` section of `$HOME/.config/belower/belowerrc`. The `{SUBCOMMAND}` is the subcommand of `belower dump`. Here's a working example:

```toml
[dump.system]
my_pattern1 = ["datetime", "os_release"]
```

The following two commands are equivalent:

```bash
$ belower dump system -b "10:00" -e "10:10" -f datetime os_release

$ belower dump system -b "10:00" -e "10:10" -p my_pattern1
```

## cmd

`belower` support customized key mapping in the `[cmd]` section of `$HOME/.config/belower/belowerrc`. Here's a working example:

```toml
[cmd]
next_tab = 'b'
prev_tab = 'ctrl-c'
next_col = 'ctrlshift-tab'
```

The customized key mapping is in format of `{COMMAND} = '{KEY}'`. The `{COMMAND}` can be found in belower helpper view (`h` or `:help`). Here's a list of supported hot keys:

```
{char}
ctrl-{char}
alt-{char}
ctrl-{KEY}
alt-{KEY}
shift-{KEY}
altshift-{KEY}
ctrlshift-{KEY}
ctrlalt-{KEY}

// {KEY} can be one of the followings
tab
enter
backspace
left
right
up
down
ins
del
home
end
page_up
page_down
esc
```

## view

`belower` support runtime view customization through the `[view]` section of `$HOME/.config/belower/belowerrc`. Here's a working example:

```toml
[view]
collapse_cgroups = true
default_view = "process"
```

Supported configuration:

* (optional)`default_view`: String, acceptable value: ["process", "cgroup", "system"] -- Indicate the user default front page
* (optional)`collapse_cgroups`: bool, acceptable value: [true, false] -- Indicate if a user want to collapse cgroup by default
* (optional)`mouse`: bool, acceptable value: [true, false], default true -- Use the mouse: click a row to select it, a tab to switch to it, or a column title to sort by it (click again to reverse), and scroll with the wheel. While belower uses the mouse, most terminals select text with Shift held down; set `mouse = false` to select text without it.
