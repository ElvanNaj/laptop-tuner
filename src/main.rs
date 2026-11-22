use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Terminal;
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const CONF: &str = "/etc/laptop-tuner.conf";
const WMI: &str = "/sys/devices/platform/asus-nb-wmi";
const KBD: &str = "/sys/class/leds/asus::kbd_backlight";
const BAT_LIMIT: &str = "/sys/class/power_supply/BAT1/charge_control_end_threshold";

// ---------------------------------------------------------------------------
// Definisi field
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum Kind {
    Int(i64, i64),
    Enum(&'static [&'static str]),
}

struct Field {
    key: &'static str,
    label: &'static str,
    section: &'static str,
    kind: Kind,
    default: &'static str,
    help: &'static str,
}

const EPP: &[&str] = &[
    "performance",
    "balance_performance",
    "default",
    "balance_powersave",
    "powersave",
];

const FIELDS: &[Field] = &[
    Field { key: "cpu_cap_khz", label: "CPU cap (kHz)", section: "CPU / FREQUENCY",
        kind: Kind::Int(800_000, 4_500_000), default: "3800000",
        help: "scaling_max_freq cap for all cores. 4500000 = full turbo (hot)." },
    Field { key: "cpu_min_khz", label: "CPU min (kHz)", section: "CPU / FREQUENCY",
        kind: Kind::Int(800_000, 4_500_000), default: "800000",
        help: "Lower frequency limit. Keep low for power-saving idle." },
    Field { key: "epp", label: "Energy preference", section: "CPU / FREQUENCY",
        kind: Kind::Enum(EPP), default: "performance",
        help: "Intel SpeedShift EPP. performance = aggressive boost." },
    Field { key: "no_turbo", label: "No turbo (0/1)", section: "CPU / FREQUENCY",
        kind: Kind::Enum(&["0", "1"]), default: "0",
        help: "1 = disable turbo. 0 = allow turbo." },
    Field { key: "max_perf_pct", label: "Max perf (%)", section: "CPU / FREQUENCY",
        kind: Kind::Int(1, 100), default: "100",
        help: "Upper limit of intel_pstate perf_pct." },
    Field { key: "min_perf_pct", label: "Min perf (%)", section: "CPU / FREQUENCY",
        kind: Kind::Int(0, 100), default: "17",
        help: "Lower limit of intel_pstate perf_pct." },
    Field { key: "hwp_dynamic_boost", label: "HWP dynamic boost", section: "CPU / FREQUENCY",
        kind: Kind::Enum(&["0", "1"]), default: "0",
        help: "Intel HWP dynamic boost." },
    Field { key: "qos_refresh", label: "Refresh thermal QoS", section: "CPU / FREQUENCY",
        kind: Kind::Enum(&["0", "1"]), default: "1",
        help: "Keep 1 on this laptop. Toggles Processor cooling devices 1->0 so\ncpufreq_cooling does not clamp scaling_max_freq to base clock." },

    Field { key: "fan_boost_mode", label: "Fan boost mode", section: "THERMAL / FAN",
        kind: Kind::Enum(&["0", "1", "2"]), default: "0",
        help: "ASUS WMI: 0=normal, 1=overboost, 2=silent." },
    Field { key: "fan_ctrl_mode", label: "Fan PWM mode", section: "THERMAL / FAN",
        kind: Kind::Enum(&["2", "0", "1"]), default: "2",
        help: "2=auto, 0=full speed, 1=manual (uses Fan PWM)." },
    Field { key: "fan_pwm", label: "Fan PWM (0-255)", section: "THERMAL / FAN",
        kind: Kind::Int(0, 255), default: "255",
        help: "Used only when Fan PWM mode = 1. 255 = 100%." },
    Field { key: "platform_profile", label: "Platform profile", section: "THERMAL / FAN",
        kind: Kind::Enum(&["quiet", "balanced", "performance"]), default: "performance",
        help: "ACPI thermal profile." },
    Field { key: "throttle_thermal_policy", label: "ASUS thermal policy", section: "THERMAL / FAN",
        kind: Kind::Enum(&["0", "1", "2", "3"]), default: "1",
        help: "0=default, 1=performance, 2=balanced, 3=silent (BIOS dependent)." },

    Field { key: "kbd_brightness", label: "Kbd backlight", section: "LIGHT / BATTERY",
        kind: Kind::Int(0, 3), default: "3",
        help: "Keyboard backlight: 0=off, 3=max (asus::kbd_backlight sysfs)." },
    Field { key: "kbd_rgb_mode", label: "RGB mode", section: "LIGHT / BATTERY",
        kind: Kind::Enum(&["0", "1", "2", "3", "4", "5", "6", "7", "8", "10", "11"]), default: "2",
        help: "ASUS aura mode. 0=static, 1=breathing, 2=flashing, 3=rainbow,\n4=wave, 5=comet, 6=reactive, 7=marquee, 8=hue, 10/11=other." },
    Field { key: "kbd_rgb_red", label: "RGB red", section: "LIGHT / BATTERY",
        kind: Kind::Int(0, 255), default: "0", help: "Red component 0-255." },
    Field { key: "kbd_rgb_green", label: "RGB green", section: "LIGHT / BATTERY",
        kind: Kind::Int(0, 255), default: "0", help: "Green component 0-255." },
    Field { key: "kbd_rgb_blue", label: "RGB blue", section: "LIGHT / BATTERY",
        kind: Kind::Int(0, 255), default: "255", help: "Blue component 0-255." },
    Field { key: "kbd_rgb_speed", label: "RGB speed", section: "LIGHT / BATTERY",
        kind: Kind::Enum(&["0", "1", "2"]), default: "0",
        help: "Effect speed: 0=slow, 1=medium, 2=fast." },
    Field { key: "kbd_rgb_save", label: "Save RGB to BIOS", section: "LIGHT / BATTERY",
        kind: Kind::Enum(&["0", "1"]), default: "0",
        help: "0=apply now only, 1=also save to BIOS (used at boot)." },
    Field { key: "charge_limit", label: "Battery limit (%)", section: "LIGHT / BATTERY",
        kind: Kind::Int(50, 100), default: "100",
        help: "Battery charge limit (charge_control_end_threshold). 80 is recommended\nfor battery health. Takes effect immediately on apply." },

    Field { key: "gpu_power_limit_w", label: "GPU power limit (W)", section: "POWER / OTHER",
        kind: Kind::Int(1, 50), default: "50",
        help: "nvidia-smi -pl. Check your GPU's supported power range." },
    Field { key: "undervolt_mv", label: "Undervolt (mV)", section: "POWER / OTHER",
        kind: Kind::Int(-150, 0), default: "0",
        help: "MSR 0x150 voltage offset (core + cache). Many BIOSes lock this; if\nreadback stays 0, the platform ignores the request." },
    Field { key: "undervolt_apply", label: "Apply undervolt", section: "POWER / OTHER",
        kind: Kind::Enum(&["0", "1"]), default: "0",
        help: "1 = apply undervolt on apply. 0 = skip." },
    Field { key: "boot_apply", label: "Apply at boot", section: "POWER / OTHER",
        kind: Kind::Enum(&["0", "1"]), default: "1",
        help: "Managed by installer: controls the /etc/rc.local entry." },
];

// ---------------------------------------------------------------------------
// Utilitas sysfs
// ---------------------------------------------------------------------------

fn read_sys(path: &str) -> String {
    fs::read_to_string(path)
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "-".to_string())
}

fn write_sys(path: &str, val: &str) -> bool {
    match OpenOptions::new().write(true).open(path) {
        Ok(mut f) => f.write_all(format!("{}\n", val).as_bytes()).is_ok(),
        Err(_) => false,
    }
}

fn cpu_freq_dirs() -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir("/sys/devices/system/cpu") {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if n.starts_with("cpu") && n[3..].chars().all(|c| c.is_ascii_digit()) {
                let p = e.path().join("cpufreq");
                if p.is_dir() {
                    out.push(p.to_string_lossy().to_string());
                }
            }
        }
    }
    out.sort();
    out
}

fn asus_hwmon() -> Option<String> {
    if let Ok(rd) = fs::read_dir("/sys/class/hwmon") {
        for e in rd.flatten() {
            let p = e.path();
            if p.join("pwm1").exists() && read_sys(&p.join("name").to_string_lossy()) == "asus" {
                return Some(p.to_string_lossy().to_string());
            }
        }
    }
    None
}

fn refresh_qos() -> usize {
    let mut n = 0;
    if let Ok(rd) = fs::read_dir("/sys/class/thermal") {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if !name.starts_with("cooling_device") {
                continue;
            }
            let p = e.path();
            if read_sys(&p.join("type").to_string_lossy()) == "Processor" {
                write_sys(&p.join("cur_state").to_string_lossy(), "1");
                std::thread::sleep(Duration::from_millis(40));
                write_sys(&p.join("cur_state").to_string_lossy(), "0");
                n += 1;
            }
        }
    }
    n
}

fn msr_write(mv: i64) -> String {
    let mut raw = (mv as f64 * 1024.0 / 1000.0).round() as i64;
    if raw < 0 {
        raw += 0x800;
    }
    let raw = (raw & 0x7ff) as u64;
    let val = raw | (raw << 11);
    let bytes = val.to_le_bytes();
    let mut written = 0;
    for cpu in 0..16 {
        let path = format!("/dev/cpu/{}/msr", cpu);
        if !Path::new(&path).exists() {
            break;
        }
        if let Ok(mut f) = OpenOptions::new().write(true).open(&path) {
            if f.seek(SeekFrom::Start(0x150)).is_ok() && f.write_all(&bytes).is_ok() {
                written += 1;
            }
        }
    }
    let readback = (|| -> io::Result<u64> {
        let mut f = OpenOptions::new().read(true).open("/dev/cpu/0/msr")?;
        f.seek(SeekFrom::Start(0x150))?;
        let mut b = [0u8; 8];
        f.read_exact(&mut b)?;
        Ok(u64::from_le_bytes(b))
    })();
    let prefix = format!("({} CPU)", written);
    match readback {
        Ok(v) if v == val => format!("undervolt{} {} mV OK (readback 0x{:x})", prefix, mv, v),
        Ok(v) => format!(
            "undervolt{} {} mV REJECTED by hardware (readback 0x{:x}, BIOS lock)",
            prefix, mv, v
        ),
        Err(e) => format!("undervolt{} read failed: {}", prefix, e),
    }
    .to_string()
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

fn default_cfg() -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    for f in FIELDS {
        m.insert(f.key.to_string(), f.default.to_string());
    }
    m
}

fn load_cfg() -> BTreeMap<String, String> {
    let mut m = default_cfg();
    if let Ok(s) = fs::read_to_string(CONF) {
        for line in s.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                m.insert(k.trim().to_string(), v.trim().to_string());
            }
        }
    }
    m
}

fn save_cfg(cfg: &BTreeMap<String, String>) -> io::Result<()> {
    let mut s = String::from("# laptop-tuner config (managed by TUI)\n");
    let mut section = "";
    for f in FIELDS {
        if f.section != section {
            section = f.section;
            s.push_str(&format!("\n# {}\n", section));
        }
        let v = cfg.get(f.key).map(String::as_str).unwrap_or(f.default);
        s.push_str(&format!("{}={}\n", f.key, v));
    }
    fs::write(CONF, s)
}

// ---------------------------------------------------------------------------
// Apply
// ---------------------------------------------------------------------------

fn apply(cfg: &BTreeMap<String, String>) -> Vec<String> {
    let mut log = Vec::new();
    let g = |k: &str| -> String { cfg.get(k).cloned().unwrap_or_default() };

    if g("qos_refresh") == "1" {
        let n = refresh_qos();
        log.push(format!("thermal QoS refresh: {} cooling device", n));
    }

    let cap = g("cpu_cap_khz").parse::<u64>().unwrap_or(3_800_000);
    let min = g("cpu_min_khz")
        .parse::<u64>()
        .unwrap_or(800_000)
        .min(cap);
    let dirs = cpu_freq_dirs();
    let mut cnt = 0;
    for d in &dirs {
        if write_sys(&format!("{}/scaling_max_freq", d), &cap.to_string())
            && write_sys(&format!("{}/scaling_min_freq", d), &min.to_string())
        {
            cnt += 1;
        }
        write_sys(
            &format!("{}/energy_performance_preference", d),
            &g("epp"),
        );
    }
    log.push(format!("CPU cap {} kHz / min {} kHz ({} core)", cap, min, cnt));

    write_sys("/sys/devices/system/cpu/intel_pstate/no_turbo", &g("no_turbo"));
    write_sys("/sys/devices/system/cpu/intel_pstate/max_perf_pct", &g("max_perf_pct"));
    write_sys("/sys/devices/system/cpu/intel_pstate/min_perf_pct", &g("min_perf_pct"));
    write_sys("/sys/devices/system/cpu/intel_pstate/hwp_dynamic_boost", &g("hwp_dynamic_boost"));
    log.push(format!(
        "EPP={} turbo={} perf={}/{} hwp_boost={}",
        g("epp"),
        g("no_turbo"),
        g("min_perf_pct"),
        g("max_perf_pct"),
        g("hwp_dynamic_boost")
    ));

    write_sys(&format!("{}/fan_boost_mode", WMI), &g("fan_boost_mode"));
    write_sys(
        &format!("{}/throttle_thermal_policy", WMI),
        &g("throttle_thermal_policy"),
    );
    write_sys("/sys/firmware/acpi/platform_profile", &g("platform_profile"));
    if let Some(h) = asus_hwmon() {
        if g("fan_ctrl_mode") == "1" {
            write_sys(&format!("{}/pwm1", h), &g("fan_pwm"));
        }
        write_sys(&format!("{}/pwm1_enable", h), &g("fan_ctrl_mode"));
        log.push(format!(
            "fan: boost={} mode={} pwm={} profile={}",
            g("fan_boost_mode"),
            g("fan_ctrl_mode"),
            g("fan_pwm"),
            g("platform_profile")
        ));
    } else {
        log.push("fan: ASUS hwmon not found".to_string());
    }

    if Path::new(&format!("{}/brightness", KBD)).exists() {
        write_sys(&format!("{}/brightness", KBD), &g("kbd_brightness"));
        let cmd = if g("kbd_rgb_save") == "1" { 1 } else { 0 };
        let rgb = format!(
            "{} {} {} {} {} {}",
            cmd,
            g("kbd_rgb_mode"),
            g("kbd_rgb_red"),
            g("kbd_rgb_green"),
            g("kbd_rgb_blue"),
            g("kbd_rgb_speed")
        );
        write_sys(&format!("{}/kbd_rgb_mode", KBD), &rgb);
        log.push(format!("keyboard: brightness={} rgb=[{}]", g("kbd_brightness"), rgb));
    }

    if Path::new(BAT_LIMIT).exists() {
        write_sys(BAT_LIMIT, &g("charge_limit"));
        log.push(format!("battery charge limit: {}%", g("charge_limit")));
    }

    let w = g("gpu_power_limit_w");
    match Command::new("nvidia-smi").args(["-pl", &w]).output() {
        Ok(o) if o.status.success() => log.push(format!("GPU power limit: {} W", w)),
        Ok(o) => log.push(format!(
            "GPU power limit failed: {}",
            String::from_utf8_lossy(&o.stderr).trim()
        )),
        Err(_) => log.push("GPU: nvidia-smi not found".to_string()),
    }

    if g("undervolt_apply") == "1" {
        let mv = g("undervolt_mv").parse::<i64>().unwrap_or(0);
        log.push(msr_write(mv));
    }
    log
}

// ---------------------------------------------------------------------------
// Monitoring
// ---------------------------------------------------------------------------

fn nvidia_query() -> String {
    match Command::new("nvidia-smi")
        .args([
            "--query-gpu=utilization.gpu,power.draw,temperature.gpu,memory.used,memory.total",
            "--format=csv,noheader,nounits",
        ])
        .output()
    {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => "-".to_string(),
    }
}

fn package_temp() -> String {
    match Command::new("sensors").output() {
        Ok(o) => {
            let s = String::from_utf8_lossy(&o.stdout);
            for line in s.lines() {
                if line.contains("Package id 0:") {
                    for tok in line.split_whitespace() {
                        let t = tok.trim_matches(|c: char| !c.is_ascii_digit() && c != '.');
                        if t.contains('.') {
                            return format!("{}C", t);
                        }
                    }
                }
            }
            "-".to_string()
        }
        _ => "-".to_string(),
    }
}

fn mem_available() -> String {
    if let Ok(s) = fs::read_to_string("/proc/meminfo") {
        for line in s.lines() {
            if line.starts_with("MemAvailable:") {
                let kb: f64 = line
                    .split_whitespace()
                    .nth(1)
                    .and_then(|x| x.parse().ok())
                    .unwrap_or(0.0);
                return format!("{:.1} GiB", kb / 1024.0 / 1024.0);
            }
        }
    }
    "-".to_string()
}

fn status_lines() -> Vec<String> {
    let d = cpu_freq_dirs();
    let f0 = d.first().map(|p| read_sys(&format!("{}/scaling_cur_freq", p))).unwrap_or_default();
    let fmax = d.first().map(|p| read_sys(&format!("{}/scaling_max_freq", p))).unwrap_or_default();
    let mhz = f0.parse::<u64>().map(|v| format!("{:.2} GHz", v as f64 / 1e6)).unwrap_or(f0);
    let maxmhz = fmax.parse::<u64>().map(|v| format!("{:.2}", v as f64 / 1e6)).unwrap_or(fmax);
    let thr = read_sys("/sys/devices/system/cpu/cpu0/thermal_throttle/package_throttle_count");
    let thr_cpu = read_sys("/sys/devices/system/cpu/cpu0/thermal_throttle/core_throttle_count");
    let fan = asus_hwmon()
        .map(|h| {
            format!(
                "mode={} pwm={}",
                read_sys(&format!("{}/pwm1_enable", h)),
                read_sys(&format!("{}/pwm1", h))
            )
        })
        .unwrap_or_else(|| "-".to_string());
    let gpu = nvidia_query();
    let gpu_parts: Vec<&str> = gpu.split(',').map(str::trim).collect();
    vec![
        format!("CPU temp      : {}", package_temp()),
        format!("CPU current   : {}", mhz),
        format!("CPU cap       : {} GHz", maxmhz),
        format!("Throttle pkg  : {}", thr),
        format!("Throttle core : {}", thr_cpu),
        format!("Fan           : {}", fan),
        format!("GPU util      : {}", gpu_parts.first().copied().unwrap_or("-")),
        format!("GPU power     : {} W", gpu_parts.get(1).copied().unwrap_or("-")),
        format!("GPU temp      : {} C", gpu_parts.get(2).copied().unwrap_or("-")),
        format!(
            "VRAM          : {} / {} MiB",
            gpu_parts.get(3).copied().unwrap_or("-"),
            gpu_parts.get(4).copied().unwrap_or("-")
        ),
        format!("RAM available : {}", mem_available()),
        format!("Platform      : {}", read_sys("/sys/firmware/acpi/platform_profile")),
        format!("Battery limit : {}%", read_sys(BAT_LIMIT)),
        format!("Kbd backlight : {}", read_sys(&format!("{}/brightness", KBD))),
    ]
}

// ---------------------------------------------------------------------------
// TUI
// ---------------------------------------------------------------------------

enum Mode {
    Normal,
    Edit(String),
    Picker { idx: usize },
    Help,
    ConfirmApply(bool),
    ConfirmDefaults(bool),
}

fn validate(kind: Kind, input: &str) -> bool {
    match kind {
        Kind::Int(lo, hi) => match input.parse::<i64>() {
            Ok(v) => v >= lo && v <= hi,
            Err(_) => false,
        },
        Kind::Enum(opts) => opts.contains(&input),
    }
}

fn cycle(kind: Kind, cur: &str, dir: i32) -> String {
    match kind {
        Kind::Int(lo, hi) => {
            let v = cur.parse::<i64>().unwrap_or(lo);
            let step = if (hi - lo) > 1000 { 100_000 } else { 1 };
            ((v + (dir as i64) * step).clamp(lo, hi)).to_string()
        }
        Kind::Enum(opts) => {
            let i = opts.iter().position(|x| *x == cur).unwrap_or(0) as i32;
            let n = opts.len() as i32;
            opts[((i + dir).rem_euclid(n)) as usize].to_string()
        }
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let p = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(p[1])[1]
}

fn ui(
    f: &mut ratatui::Frame,
    cfg: &BTreeMap<String, String>,
    sel: usize,
    top: usize,
    mode: &Mode,
    msg: &str,
    status: &[String],
) {
    let area = f.area();
    f.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title(" LAPTOP-TUNER - ASUS Laptop ")
            .border_style(Style::default().fg(Color::Cyan)),
        area,
    );
    let inner = Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(3),
        ])
        .split(inner);

    let title = Paragraph::new(Line::from(vec![
        Span::styled(" Set ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::raw("=edit  "),
        Span::styled("↑↓", Style::default().fg(Color::Green)),
        Span::raw("=select  "),
        Span::styled("a", Style::default().fg(Color::Green)),
        Span::raw("=apply  "),
        Span::styled("s", Style::default().fg(Color::Green)),
        Span::raw("=save  "),
        Span::styled("d", Style::default().fg(Color::Green)),
        Span::raw("=default  "),
        Span::styled("h", Style::default().fg(Color::Green)),
        Span::raw("=help  "),
        Span::styled("q", Style::default().fg(Color::Green)),
        Span::raw("=quit"),
    ]));
    f.render_widget(title, chunks[0]);

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(68), Constraint::Percentage(32)])
        .split(chunks[1]);

    // --- daftar field (dengan scroll) ---
    #[derive(Clone)]
    enum Row {
        Head(&'static str),
        Field(usize),
    }
    let mut rows: Vec<Row> = Vec::new();
    let mut last_sec = "";
    for (i, fld) in FIELDS.iter().enumerate() {
        if fld.section != last_sec {
            rows.push(Row::Head(fld.section));
            last_sec = fld.section;
        }
        rows.push(Row::Field(i));
    }
    let sel_row = rows
        .iter()
        .position(|r| matches!(r, Row::Field(i) if *i == sel))
        .unwrap_or(0);
    let view_h = body[0].height.saturating_sub(2) as usize;
    let top = top.min(sel_row.saturating_sub(view_h.saturating_sub(1)));
    let mut lines: Vec<Line> = Vec::new();
    for row in rows.iter().skip(top).take(view_h) {
        match row {
            Row::Head(s) => lines.push(Line::from(Span::styled(
                format!(" {}", s),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ))),
            Row::Field(i) => {
                let fld = &FIELDS[*i];
                let v = cfg.get(fld.key).map(String::as_str).unwrap_or(fld.default);
                let shown = match mode {
                    Mode::Edit(ref buf) if *i == sel => buf.clone(),
                    _ => v.to_string(),
                };
                let live = live_value(fld.key);
                let is_sel = *i == sel;
                let prefix = if is_sel { "▶" } else { " " };
                let style = if is_sel {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                lines.push(Line::from(vec![
                    Span::styled(format!("{} {:<24}", prefix, fld.label), style),
                    Span::styled(format!(" {:<14}", shown), style),
                    Span::styled(
                        format!(" [live: {}]", live),
                        if is_sel {
                            style
                        } else {
                            Style::default().fg(Color::DarkGray)
                        },
                    ),
                ]));
            }
        }
    }
    f.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" SETTINGS ")),
        body[0],
    );

    // --- panel monitoring ---
    let s: Vec<Line> = status
        .iter()
        .map(|x| Line::from(Span::raw(x.clone())))
        .collect();
    f.render_widget(
        Paragraph::new(s)
            .block(Block::default().borders(Borders::ALL).title(" MONITOR "))
            .wrap(Wrap { trim: false }),
        body[1],
    );

    // --- footer / pesan ---
    let tip = match mode {
        Mode::Edit(_) => " Type value, Enter=save, Esc=cancel ".to_string(),
        Mode::Picker { .. } => " ↑↓ = select, Enter = use, e = type manually, Esc = cancel ".to_string(),
        Mode::Help => " Help ".to_string(),
        Mode::ConfirmApply(_) => " Apply all changes? [y/N] ".to_string(),
        Mode::ConfirmDefaults(_) => " Restore all defaults? [y/N] ".to_string(),
        Mode::Normal => format!(" {} | {}", msg, FIELDS[sel].help.replace('\n', " ")),
    };
    let foot_style = if msg.contains("FAILED") || msg.contains("REJECTED") {
        Style::default().fg(Color::Red)
    } else {
        Style::default().fg(Color::Green)
    };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(tip, foot_style)))
            .block(Block::default().borders(Borders::ALL).title(" STATUS ")),
        chunks[2],
    );

    // --- overlay pilihan enum ---
    if let Mode::Picker { idx } = mode {
        let fld = &FIELDS[sel];
        let opts = match fld.kind {
            Kind::Enum(o) => o,
            Kind::Int(_, _) => &[][..],
        };
        let h = (opts.len() as u16 + 4).min(area.height.saturating_sub(4));
        let r = centered_rect(45, 0, area);
        let r = Rect {
            height: h,
            y: r.y.saturating_sub(h / 2),
            ..r
        };
        f.render_widget(Clear, r);
        let lines: Vec<Line> = opts
            .iter()
            .enumerate()
            .map(|(i, o)| {
                let on = i == *idx;
                Line::from(Span::styled(
                    format!(" {} {}", if on { "▶" } else { " " }, o),
                    if on {
                        Style::default()
                            .fg(Color::Black)
                            .bg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                    },
                ))
            })
            .collect();
        f.render_widget(
            Paragraph::new(lines).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" SELECT: {} ", fld.label))
                    .border_style(Style::default().fg(Color::Cyan)),
            ),
            r,
        );
    }

    // --- overlay bantuan ---
    if matches!(mode, Mode::Help) {
        let r = centered_rect(70, 70, area);
        f.render_widget(Clear, r);
        let help: Vec<Line> = vec![
            Line::from(Span::styled(" HELP", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))),
            Line::from(""),
            Line::from(" ↑ / ↓       : move between fields"),
            Line::from(" Enter       : pick from list (enum) / type value (int)"),
            Line::from(" ← / →       : increase/decrease value (step)"),
            Line::from(" a           : apply ALL settings (requires sudo)"),
            Line::from(" s           : save to /etc/laptop-tuner.conf"),
            Line::from(" d           : restore all defaults"),
            Line::from(" h           : close this help"),
            Line::from(" q / Esc     : quit"),
            Line::from(""),
            Line::from(Span::styled(" Important notes:", Style::default().fg(Color::Yellow))),
            Line::from(" - keep thermal QoS refresh enabled on this platform"),
            Line::from(" - undervolt may be BIOS-locked (MSR 0x150 ignored)"),
            Line::from(" - high temps under load are usually the power/thermal limit"),
        ];
        f.render_widget(
            Paragraph::new(help)
                .block(Block::default().borders(Borders::ALL).title(" HELP ").border_style(Style::default().fg(Color::Cyan)))
                .wrap(Wrap { trim: false }),
            r,
        );
    }
}

fn live_value(key: &str) -> String {
    let d = cpu_freq_dirs();
    let c0 = d.first().cloned().unwrap_or_default();
    match key {
        "cpu_cap_khz" => read_sys(&format!("{}/scaling_max_freq", c0)),
        "cpu_min_khz" => read_sys(&format!("{}/scaling_min_freq", c0)),
        "epp" => read_sys(&format!("{}/energy_performance_preference", c0)),
        "no_turbo" | "max_perf_pct" | "min_perf_pct" | "hwp_dynamic_boost" => {
            read_sys(&format!("/sys/devices/system/cpu/intel_pstate/{}", key))
        }
        "qos_refresh" => {
            let mut n = 0;
            if let Ok(rd) = fs::read_dir("/sys/class/thermal") {
                for e in rd.flatten() {
                    let p = e.path();
                    if read_sys(&p.join("type").to_string_lossy()) == "Processor" {
                        n += 1;
                    }
                }
            }
            format!("{} device", n)
        }
        "fan_boost_mode" | "throttle_thermal_policy" => {
            read_sys(&format!("{}/{}", WMI, key))
        }
        "fan_ctrl_mode" | "fan_pwm" => asus_hwmon()
            .map(|h| {
                let f = if key == "fan_pwm" { "pwm1" } else { "pwm1_enable" };
                read_sys(&format!("{}/{}", h, f))
            })
            .unwrap_or_default(),
        "platform_profile" => read_sys("/sys/firmware/acpi/platform_profile"),
        "kbd_brightness" => read_sys(&format!("{}/brightness", KBD)),
        "kbd_rgb_mode" | "kbd_rgb_speed" => "- (write-only)".to_string(),
        "kbd_rgb_red" | "kbd_rgb_green" | "kbd_rgb_blue" => "- (write-only)".to_string(),
        "kbd_rgb_save" => "-".to_string(),
        "charge_limit" => read_sys(BAT_LIMIT),
        "gpu_power_limit_w" => {
            let out = Command::new("nvidia-smi")
                .args(["--query-gpu=power.limit", "--format=csv,noheader,nounits"])
                .output();
            out.ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_default()
        }
        "undervolt_mv" | "undervolt_apply" => {
            let v = fs::File::open("/dev/cpu/0/msr").and_then(|mut f| {
                f.seek(SeekFrom::Start(0x150))?;
                let mut b = [0u8; 8];
                f.read_exact(&mut b)?;
                Ok(u64::from_le_bytes(b))
            });
            match v {
                Ok(x) if x == 0 => "0 (locked?)".to_string(),
                Ok(x) => format!("0x{:x}", x),
                Err(_) => "-".to_string(),
            }
        }
        "boot_apply" => read_sys("/etc/rc.local")
            .contains("laptop-tuner")
            .to_string(),
        _ => "-".to_string(),
    }
}

fn run_tui() -> Result<(), Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    let mut out = io::stdout();
    execute!(out, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(out);
    let mut term = Terminal::new(backend)?;
    let mut cfg = load_cfg();
    let mut sel = 0usize;
    let top = 0usize;
    let mut mode = Mode::Normal;
    let mut msg = String::from(" Ready. Select a field, then Enter to change.");

    // Monitoring dijalankan thread terpisah agar tomato tidak menunggu fork
    // `sensors` / `nvidia-smi`. UI hanya membaca hasil cache.
    let status: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(status_lines()));
    let status_bg = Arc::clone(&status);
    thread::spawn(move || loop {
        thread::sleep(Duration::from_millis(800));
        let s = status_lines();
        if let Ok(mut guard) = status_bg.lock() {
            *guard = s;
        }
    });

    loop {
        let snap = status.lock().map(|g| g.clone()).unwrap_or_default();
        term.draw(|f| ui(f, &cfg, sel, top, &mode, &msg, &snap))?;
        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                match &mut mode {
                    Mode::Edit(buf) => match key.code {
                        KeyCode::Char(c) => {
                            buf.push(c);
                        }
                        KeyCode::Backspace => {
                            buf.pop();
                        }
                        KeyCode::Enter => {
                            let fld = &FIELDS[sel];
                            if validate(fld.kind, buf) {
                                cfg.insert(fld.key.to_string(), buf.clone());
                                msg = format!(" {} = {}", fld.key, buf);
                            } else {
                                msg = format!(" FAILED: invalid value for {}", fld.label);
                            }
                            mode = Mode::Normal;
                        }
                        KeyCode::Esc => mode = Mode::Normal,
                        _ => {}
                    },
                    Mode::Picker { idx } => {
                        let fld = &FIELDS[sel];
                        let opts = match fld.kind {
                            Kind::Enum(o) => o,
                            Kind::Int(_, _) => &[][..],
                        };
                        match key.code {
                            KeyCode::Down => *idx = (*idx + 1).min(opts.len().saturating_sub(1)),
                            KeyCode::Up => *idx = idx.saturating_sub(1),
                            KeyCode::Enter => {
                                if let Some(v) = opts.get(*idx) {
                                    cfg.insert(fld.key.to_string(), v.to_string());
                                    msg = format!(" {} = {}", fld.key, v);
                                }
                                mode = Mode::Normal;
                            }
                            KeyCode::Char('e') | KeyCode::Char('E') => {
                                let v = cfg
                                    .get(fld.key)
                                    .cloned()
                                    .unwrap_or_else(|| fld.default.to_string());
                                mode = Mode::Edit(v);
                            }
                            KeyCode::Esc => mode = Mode::Normal,
                            _ => {}
                        }
                    }
                    Mode::Help => {
                        if matches!(key.code, KeyCode::Esc | KeyCode::Char('h') | KeyCode::Enter) {
                            mode = Mode::Normal;
                        }
                    }
                    Mode::ConfirmApply(yes) | Mode::ConfirmDefaults(yes) => {
                        match key.code {
                            KeyCode::Char('y') | KeyCode::Char('Y') => *yes = true,
                            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => *yes = false,
                            _ => {}
                        }
                        let confirmed = *yes;
                        let is_apply = matches!(mode, Mode::ConfirmApply(_));
                        if matches!(key.code, KeyCode::Enter | KeyCode::Char(_) | KeyCode::Esc) {
                            if confirmed {
                                if is_apply {
                                    let log = apply(&cfg);
                                    if let Ok(mut guard) = status.lock() {
                                        *guard = status_lines();
                                    }
                                    msg = format!(" APPLIED: {}", log.join(" | "));
                                } else {
                                    cfg = default_cfg();
                                    msg = " Defaults restored (not applied yet)".to_string();
                                }
                            } else {
                                msg = " Cancelled".to_string();
                            }
                            mode = Mode::Normal;
                        }
                    }
                    Mode::Normal => match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => break,
                        KeyCode::Down => {
                            sel = (sel + 1).min(FIELDS.len() - 1);
                        }
                        KeyCode::Up => sel = sel.saturating_sub(1),
                        KeyCode::PageDown => sel = (sel + 6).min(FIELDS.len() - 1),
                        KeyCode::PageUp => sel = sel.saturating_sub(6),
                        KeyCode::Enter => {
                            let fld = &FIELDS[sel];
                            let v = cfg
                                .get(fld.key)
                                .cloned()
                                .unwrap_or_else(|| fld.default.to_string());
                            mode = match fld.kind {
                                Kind::Enum(opts) => Mode::Picker {
                                    idx: opts.iter().position(|x| *x == v).unwrap_or(0),
                                },
                                Kind::Int(_, _) => Mode::Edit(v),
                            };
                        }
                        KeyCode::Right | KeyCode::Left => {
                            let dir = if key.code == KeyCode::Right { 1 } else { -1 };
                            let fld = &FIELDS[sel];
                            let cur = cfg
                                .get(fld.key)
                                .cloned()
                                .unwrap_or_else(|| fld.default.to_string());
                            let nv = cycle(fld.kind, &cur, dir);
                            cfg.insert(fld.key.to_string(), nv.clone());
                            msg = format!(" {} = {}", fld.key, nv);
                        }
                        KeyCode::Char('a') => mode = Mode::ConfirmApply(false),
                        KeyCode::Char('d') => mode = Mode::ConfirmDefaults(false),
                        KeyCode::Char('s') => match save_cfg(&cfg) {
                            Ok(_) => msg = format!(" Saved to {}", CONF),
                            Err(e) => msg = format!(" FAILED to save: {}", e),
                        },
                        KeyCode::Char('h') | KeyCode::Char('?') => mode = Mode::Help,
                        _ => {}
                    },
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 && args[1] == "--apply" {
        let cfg = load_cfg();
        for line in apply(&cfg) {
            println!("{}", line);
        }
        return;
    }
    if args.len() > 1 && args[1] == "--status" {
        for l in status_lines() {
            println!("{}", l);
        }
        return;
    }
    if let Err(e) = run_tui() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
