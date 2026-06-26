#!/bin/bash

# Exit on error
set -e

echo "Building zenbookd..."
cargo build --release

echo "Installing binaries..."
sudo cp target/release/zenbookd-service /usr/local/bin/
sudo cp target/release/zenbookd /usr/local/bin/

echo "Setting up configuration and state..."
sudo mkdir -p /etc/zenbookd
sudo mkdir -p /var/lib/zenbookd

if [ ! -f /etc/zenbookd/config.toml ]; then
    sudo tee /etc/zenbookd/config.toml > /dev/null <<EOF
# zenbookd configuration

# The charge limit in percentage between 0-100.
charge_limit = 80

# Whether to periodically charge to 100% to calibrate the BMS.
enable_periodic_full_charge = true

# The period in days for the full charge.
full_charge_period = 30
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

echo "Installing systemd service..."
sudo cp scripts/zenbookd.service /etc/systemd/system/

sudo systemctl daemon-reload
sudo systemctl enable --now zenbookd.service

echo "zenbookd has been installed and started."
echo "You can check the service status with: systemctl status zenbookd.service"
echo "You can use the CLI tool with: zenbookd status"
