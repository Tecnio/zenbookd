#!/bin/bash

# Exit on error
set -e

echo "Stopping and disabling systemd service..."
sudo systemctl disable --now zenbookd.service || true

echo "Removing systemd service file..."
sudo rm -f /etc/systemd/system/zenbookd.service
sudo systemctl daemon-reload

echo "Removing binaries..."
sudo rm -f /usr/local/bin/zenbookd-service
sudo rm -f /usr/local/bin/zenbookd

echo "Removing configuration and state..."
echo "Note: /etc/zenbookd and /var/lib/zenbookd are not removed to preserve your settings and state."
echo "If you want to remove them, run: rm -rf /etc/zenbookd /var/lib/zenbookd"

echo "zenbookd has been uninstalled."
