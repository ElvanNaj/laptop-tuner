#!/bin/bash
# Installer for laptop-tuner: a Rust TUI for tuning ASUS laptop
# performance, thermals, keyboard lighting and battery charge limit.
# Run: sudo ./install.sh
set -e
[ "$(id -u)" -eq 0 ] || { echo "Run with sudo" >&2; exit 1; }

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SRC="$SCRIPT_DIR/target/release/laptop-tuner"
DST=/usr/local/sbin/laptop-tuner
CONF=/etc/laptop-tuner.conf
RC=/etc/rc.local

[ -x "$SRC" ] || { echo "Build first: cargo build --release" >&2; exit 1; }

echo "=== 1. Install binary ==="
install -m 0755 "$SRC" "$DST"
echo "Installed $DST ($(stat -c %s "$DST") bytes)"

echo
echo "=== 2. Install 'ccontrol' shortcut ==="
install -m 0755 "$SCRIPT_DIR/ccontrol" /usr/local/bin/ccontrol
echo "Installed /usr/local/bin/ccontrol"

echo
echo "=== 3. Config ==="
if [ ! -f "$CONF" ]; then
    cat > "$CONF" <<'EOF'
# laptop-tuner config (managed by TUI)

# CPU / FREQUENCY
cpu_cap_khz=3800000
cpu_min_khz=800000
epp=performance
no_turbo=0
max_perf_pct=100
min_perf_pct=17
hwp_dynamic_boost=0
qos_refresh=1

# THERMAL / FAN
fan_boost_mode=0
fan_ctrl_mode=2
fan_pwm=255
platform_profile=performance
throttle_thermal_policy=1

# LIGHT / BATTERY
kbd_brightness=3
kbd_rgb_mode=2
kbd_rgb_red=0
kbd_rgb_green=0
kbd_rgb_blue=255
kbd_rgb_speed=0
kbd_rgb_save=0
charge_limit=100

# POWER / OTHER
gpu_power_limit_w=50
undervolt_mv=0
undervolt_apply=0
boot_apply=1
EOF
    echo "Created $CONF with defaults."
else
    echo "$CONF already exists, not overwriting."
fi

echo
echo "=== 4. Boot hook ==="
if [ -f "$RC" ] && ! command -v systemctl >/dev/null 2>&1; then
    cp "$RC" "${RC}.bak-laptop-tuner" 2>/dev/null || true
    sed -i '\#/usr/local/sbin/laptop-tuner --apply#d' "$RC"
    sed -i "\#^exit 0#i $DST --apply >/var/log/laptop-tuner.log 2>&1" "$RC"
    echo "Added rc.local entry:"
    tail -4 "$RC"
elif command -v systemctl >/dev/null 2>&1 && systemctl is-system-running >/dev/null 2>&1; then
    cat > /etc/systemd/system/laptop-tuner.service <<'UNIT'
[Unit]
Description=Apply laptop-tuner settings at boot
After=multi-user.target

[Service]
Type=oneshot
ExecStart=/usr/local/sbin/laptop-tuner --apply

[Install]
WantedBy=multi-user.target
UNIT
    systemctl daemon-reload
    systemctl enable --now laptop-tuner.service
    echo "Systemd service enabled."
else
    echo "WARNING: no supported init hook found; apply manually at boot."
fi

echo
echo "=== 5. Apply now ==="
"$DST" --apply

echo
echo "=== 6. Status ==="
"$DST" --status

echo
echo "Done. Open the TUI with:"
echo "  ccontrol"
