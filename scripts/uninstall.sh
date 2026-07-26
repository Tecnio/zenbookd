#!/bin/bash

# Exit on error
set -e

echo "Stopping and disabling systemd service..."
sudo systemctl disable --now zenbookd.service || true

echo "Removing systemd service file..."
sudo rm -f /etc/systemd/system/zenbookd.service
sudo systemctl daemon-reload

echo "Removing udev rule..."
sudo rm -f /etc/udev/rules.d/99-zenbookd-battery.rules
sudo udevadm control --reload || true

echo "Removing binaries..."
sudo rm -f /usr/local/bin/zenbookd-service
sudo rm -f /usr/local/bin/zenbookd

echo "Note: system user zenbookd is left in place."
echo "Note: /etc/zenbookd and /var/lib/zenbookd are not removed to preserve your settings and state."
echo "If you want to remove them, run:"
echo "  sudo userdel zenbookd"
echo "  sudo rm -rf /etc/zenbookd /var/lib/zenbookd"

echo "zenbookd has been uninstalled."
