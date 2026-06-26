# zenbookd
> Daemon and CLI tool for my Zenbook's battery charge limit

Because Linux is a dysfunctional desktop experience I cannot have a charge limit applied in a matter that isn't a JavaScript vibe slop shell extension or a non-persistent setting in a shitty settings page so here we are. 

## Install

```sh
./scripts/install.sh
```

## Usage

```sh
zenbookd status            # current charge, health and config
zenbookd set-limit 80      # hold the battery at 80%
zenbookd boost             # charge to 100% now, restore the limit after
zenbookd boost --stop      # cancel an active boost early
```

## Config

`/etc/zenbookd/config.toml`:

```toml
# Percentage (0-100) the battery is held at.
charge_limit = 80

# Periodically charge to 100% to let the BMS recalibrate.
enable_periodic_full_charge = true

# How often that full charge happens, in days.
full_charge_period = 30
```

Changes made through the CLI are written back here, so edits and commands stay in
sync. The daemon keeps its own state (last full charge, any active boost) in
`/var/lib/zenbookd/state.toml`.

## License

MIT, see [LICENSE.md](LICENSE.md).
