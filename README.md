# laptop-tuner

A small Rust TUI for tuning an ASUS laptop from Linux: CPU frequency and
energy settings, thermal/fan behaviour, NVIDIA GPU power limit, keyboard
backlight/RGB and battery charge limit.

It was written for an ASUS laptop with an Intel CPU and an NVIDIA GPU running
**SysV init**, but it degrades gracefully: the installer uses a systemd unit
when systemd is running, otherwise it falls back to `/etc/rc.local`.

## Why this exists

On some ASUS laptops the kernel's `cpufreq_cooling` thermal driver latches a
`FREQ_QOS_MAX` constraint at the CPU base clock during boot. The result is
that `no_turbo=0` and `max_perf_pct=100` are ignored and `scaling_max_freq`
stays pinned at the base frequency (e.g. 2.5 GHz). Writes to `scaling_max_freq`
are reverted immediately.

`laptop-tuner` fixes this by toggling every `Processor` cooling device from
`cur_state 1 -> 0`, which forces the kernel to re-write the QoS request at the
highest frequency. The desired cap is then applied normally. Keeping
`qos_refresh=1` makes this happen at every boot.

## Features

| Group | Settings |
| --- | --- |
| CPU / Frequency | `scaling_max_freq`, `scaling_min_freq`, Energy Performance Preference, `no_turbo`, `max_perf_pct`, `min_perf_pct`, HWP dynamic boost, thermal QoS refresh |
| Thermal / Fan | ASUS fan boost mode (normal/overboost/silent), PWM mode (auto/full/manual) and manual PWM value, ACPI platform profile, ASUS thermal policy |
| Light / Battery | Keyboard backlight level, keyboard RGB mode/colour/speed/save-to-BIOS, battery charge limit |
| Power / Other | NVIDIA GPU power limit (`nvidia-smi -pl`), CPU undervolt via MSR `0x150` |
| Monitoring | Live CPU temperature, current/cap frequency, throttle counters, fan state, GPU utilisation/power/temperature, available RAM, battery limit |

The TUI runs monitoring in a background thread, so holding arrow keys stays
responsive and the status panel refreshes on its own.

Enum fields use a list picker (arrow keys + Enter). Press `e` to type a raw
value instead. Numeric fields accept any value inside their range.

## Requirements

* Linux with `intel_pstate` (tested on Intel Comet Lake)
* Root access (writes to `/sys` and `/dev/cpu/*/msr`)
* Rust toolchain only if building from source
* `sensors` (lm-sensors) and `nvidia-smi` for the monitoring panel
* ASUS WMI / hwmon nodes for fan and keyboard settings

Unavailable features are skipped with a warning rather than causing failure.
For example, undervolt is ignored when the BIOS locks MSR `0x150`, and panel
overdrive is skipped when no `panel_od` sysfs node exists.

## Build and install

```bash
cargo build --release
sudo ./install.sh
```

The installer:

1. installs the binary to `/usr/local/sbin/laptop-tuner`
2. installs the `ccontrol` shortcut to `/usr/local/bin/ccontrol`
3. creates `/etc/laptop-tuner.conf` if it does not exist
4. registers a boot hook (systemd unit or `/etc/rc.local`)
5. applies the config immediately

## Usage

```bash
ccontrol              # interactive TUI
ccontrol --status     # one-shot status readout
ccontrol --apply      # apply saved config without the TUI
```

TUI keys:

| Key | Action |
| --- | --- |
| `↑` / `↓` | move between fields |
| `Enter` | open picker (enum) or edit value (number) |
| `←` / `→` | step value up/down |
| `e` | type a raw value while a picker is open |
| `a` | apply all changes to the system |
| `s` | save config to `/etc/laptop-tuner.conf` |
| `d` | restore defaults |
| `h` | help |
| `q` / `Esc` | quit |

## Configuration

`/etc/laptop-tuner.conf` is a plain `KEY=VALUE` file. Missing keys fall back
to built-in defaults, so new fields can be added without breaking an existing
config. Every value is an independent variable; there are no hidden presets.

| Key | Default | Meaning |
| --- | --- | --- |
| `cpu_cap_khz` | `3800000` | Maximum CPU frequency in kHz |
| `cpu_min_khz` | `800000` | Minimum CPU frequency in kHz |
| `epp` | `performance` | Intel SpeedShift EPP |
| `no_turbo` | `0` | Set to `1` to disable turbo |
| `max_perf_pct` / `min_perf_pct` | `100` / `17` | `intel_pstate` limits |
| `hwp_dynamic_boost` | `0` | Intel HWP dynamic boost |
| `qos_refresh` | `1` | Refresh the thermal `FREQ_QOS_MAX` constraint |
| `fan_boost_mode` | `0` | `0` normal, `1` overboost, `2` silent |
| `fan_ctrl_mode` | `2` | `2` auto, `0` full speed, `1` manual |
| `fan_pwm` | `255` | Manual fan duty cycle (0-255) |
| `platform_profile` | `performance` | ACPI platform profile |
| `throttle_thermal_policy` | `1` | ASUS thermal policy |
| `kbd_brightness` | `3` | Keyboard backlight (0-3) |
| `kbd_rgb_*` | see config | Keyboard RGB mode, colour, speed, save flag |
| `charge_limit` | `100` | Battery charge end threshold (%) |
| `gpu_power_limit_w` | `50` | NVIDIA GPU power limit in watts |
| `undervolt_mv` | `0` | MSR `0x150` offset in mV |
| `undervolt_apply` | `0` | Set to `1` to actually write the undervolt |
| `boot_apply` | `1` | Informational: boot hook managed by installer |

## Notes and limitations

* Sustained CPU frequency is still bounded by power and thermals. Raising the
  cap does not bypass the power limit.
* `nvidia-smi -pl` is limited to the range advertised by the driver.
* Keyboard RGB writes use the ASUS WMI interface. On models with only a
  white backlight, colour changes may not be visible even if the write
  succeeds.
* Some BIOS versions lock undervolt MSRs. The tool reports this instead of
  failing.

## Uninstall

Remove `/usr/local/sbin/laptop-tuner`, `/usr/local/bin/ccontrol` and the boot
entry (`/etc/systemd/system/laptop-tuner.service` or the line in
`/etc/rc.local`), then delete `/etc/laptop-tuner.conf` if desired.

## License

MIT
