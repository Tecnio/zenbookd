#!/bin/bash

# Exit on error
set -e

echo "Building zenbookd..."
RUSTFLAGS="-C target-cpu=native" cargo build --release

echo "Installing binaries..."
sudo cp target/release/zenbookd-service /usr/local/bin/
sudo cp target/release/zenbookd /usr/local/bin/

echo "Ensuring zenbookd system user exists..."
if ! id zenbookd &>/dev/null; then
    sudo useradd --system --no-create-home \
        --home-dir /var/lib/zenbookd \
        --shell /usr/sbin/nologin \
        --comment "ASUS Zenbook battery daemon" \
        zenbookd

    echo "Created system user zenbookd"
else
    echo "System user zenbookd already exists"
fi

echo "Setting up configuration and state..."
sudo mkdir -p /etc/zenbookd
sudo mkdir -p /var/lib/zenbookd

if [ ! -f /etc/zenbookd/config.toml ]; then
    sudo tee /etc/zenbookd/config.toml > /dev/null <<EOF
# zenbookd configuration

# The charge limit in percentage between 1-100.
charge_limit = 80

# Whether to periodically charge to 100% to calibrate the BMS.
enable_periodic_full_charge = true

# The period in days for the full charge.
full_charge_period = 30

# When enabled, Wi-Fi power saving is disabled while on AC power.
disable_wifi_power_save_on_ac = true
EOF
    echo "Created default configuration at /etc/zenbookd/config.toml"
else
    echo "Configuration file already exists at /etc/zenbookd/config.toml"
fi

if [ ! -f /var/lib/zenbookd/state.toml ]; then
    CURRENT_DATE=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
    sudo tee /var/lib/zenbookd/state.toml > /dev/null <<EOF
last_full_charge = "$CURRENT_DATE"
EOF
    echo "Initialized battery state at /var/lib/zenbookd/state.toml"
fi

sudo chown -R zenbookd:zenbookd /etc/zenbookd /var/lib/zenbookd

TARGET_USER="${SUDO_USER:-$USER}"

if [ "$TARGET_USER" != "root" ]; then
    if id -nG "$TARGET_USER" | tr ' ' '\n' | grep -qx zenbookd; then
        echo "User $TARGET_USER is already in the zenbookd group"
    else
        sudo usermod -aG zenbookd "$TARGET_USER"
        echo "Added $TARGET_USER to the zenbookd group"
        NEEDS_RELOGIN=1
    fi
fi

echo "Installing udev rule for charge threshold access..."
sudo cp scripts/99-zenbookd-battery.rules /etc/udev/rules.d/
sudo udevadm control --reload
sudo udevadm trigger -c add -s power_supply

echo "Installing systemd service..."
sudo cp scripts/zenbookd.service /etc/systemd/system/

sudo systemctl daemon-reload
sudo systemctl enable --now zenbookd.service

echo "zenbookd has been installed and started (running as user zenbookd)."
echo "You can check the service status with: systemctl status zenbookd.service"
echo "You can use the CLI tool with: zenbookd status"

if [ -n "${NEEDS_RELOGIN:-}" ]; then
    echo
    echo "The CLI talks to the daemon over a socket owned by the zenbookd group."
    echo "Log back in (or run 'newgrp zenbookd') before using it in this shell."
fi
