# eh - Ergonomic Nix CLI Helper

[intelligent error handling]: #auto-retry-with-environment-variables
[automatic fixes]: #hash-auto-fix

`eh` is a multicall [^1] (think Busybox) CLI tool that provides _ergonomic_
shortcuts for common Nix commands with [intelligent error handling], and more
importantly, [automatic fixes] for common Nix errors.

[^1]: See [aliases](#shell-aliases) section.

## Features

We provide a very tiny binary with a limited set of features. The most critical
features are **automatic hash fixes** and **auto-retry with appropriate
environment variables**.

### Auto-Retry with Environment Variables

When a package fails to build due to licensing, security, stability, or whatever
reason provided by Nixpkgs, `eh` equivalent of Nix commands retry automatically
with the appropriate environment variables. While this is called automatic
_retry_, we collect information about the packages from the `meta` field instead
of building the package. The following variables are supported:

- **Unfree packages**: Sets `NIXPKGS_ALLOW_UNFREE=1`
- **Insecure packages**: Sets `NIXPKGS_ALLOW_INSECURE=1`
- **Broken packages**: Sets `NIXPKGS_ALLOW_BROKEN=1`

Auto-retry requires that `--impure` is not explicitly disabled for the relevant
command in the config file. By default retries are automatic. See
[Configuration](#configuration).

### Hash Auto-Fix

When a hash mismatch is detected in the underlying `nix build`, `eh` can
automatically update the old, broken hash with a new and correct one _directly
in the source file_.

## Shell Aliases

By default, you may run the `eh` binary akin to Nix with a nicer interface. The
supported Nix commands, i.e., nix `build`, `shell`, `run` and `develop` become
`eh build`, `eh shell`, `eh run` and `eh develop`. However, it is possible to
symlink the `eh` binary to `nb`, `ns`, `nr`, and `nd` to invoke a specific
feature. For example, `nb` will act as `eh build` and `nr` will be `eh run`.

One special example is `eh update`, which is aliases to `nu`, that handles
interactive Nix flake updates. It is special in the sense that the usage is
entirely different from its Nix counterpart, where you get to _interactively_
pick which inputs to update.

After enabling shell aliases via the NixOS module or Home Manager, you can use:

```bash
ns nixpkgs#hello           # equivalent to: nix shell nixpkgs#hello
nr nixpkgs#cowsay "Hello!" # nix run nixpkgs#cowsay
nb .#myPackage             # nix build .#myPackage
nu                         # nix flake update
```

## Configuration

`eh` reads configuration from the first `.eh.toml` found by walking up from the
current directory, falling back to `~/.config/eh/config.toml`. If no file
exists, all defaults apply and no extra flags are passed to Nix.

### Global settings

Top-level keys apply to every command unless overridden per-command:

```toml
# Explicitly enable --impure for all commands (also passes it on initial run).
impure = true

# Explicitly disable impure retries for all commands.
impure = false
```

When `impure` is absent (the default), auto-retry with `--impure` is
**automatic** — `eh` will add `--impure` and the appropriate `NIXPKGS_ALLOW_*`
variable whenever it detects an unfree, insecure, or broken package.

<!--markdownlint-disable MD013-->

| Key      | Type | Default | Description                                                    |
| -------- | ---- | ------- | -------------------------------------------------------------- |
| `impure` | bool | -       | `true` passes `--impure` always; `false` blocks impure retries |

<!--markdownlint-enable MD013-->

### Per-command settings

Each command can be configured independently under `[commands.<name>]`. A
per-command setting takes precedence over the global one; the global setting
applies to commands that do not have their own entry.

```toml
[commands.build]
impure = true
env = { NIXPKGS_ALLOW_UNFREE = "1" }

[commands.develop]
impure = false

[commands.develop.env]
MY_DEV_VAR = "1"
```

<!--markdownlint-disable MD013-->

| Key      | Type  | Default | Description                                                                     |
| -------- | ----- | ------- | ------------------------------------------------------------------------------- |
| `impure` | bool  | -       | `true` passes `--impure` always; `false` blocks impure retries for this command |
| `env`    | table | `{}`    | Extra environment variables to set for the command                              |

<!--markdownlint-enable MD013-->

### Impure mode and unfree/insecure/broken packages

When `eh` detects that a package requires `--impure` (unfree, insecure, or
broken), it retries automatically with the appropriate `NIXPKGS_ALLOW_*`
variable and `--impure` by default.

If `impure = false` is set for the active command (or globally), the retry is
blocked and an error is shown instead:

```plaintext
! package has an unfree license but `--impure` is disabled for `build` in config
~ set `impure = true` for this command (or globally) in .eh.toml or
  ~/.config/eh/config.toml, or pass `--impure` manually
```

To explicitly enable `--impure` for a specific command (also adds it to the
initial run, not just retries):

```toml
[commands.build]
impure = true
```

To disable impure retries globally:

```toml
impure = false
```

## License

<!--markdownlint-disable MD059-->

This project is made available under Mozilla Public License (MPL) version 2.0.
See [LICENSE](LICENSE) for more details on the exact conditions. An online copy
is provided [here](https://www.mozilla.org/en-US/MPL/2.0/).

<!--markdownlint-enable MD059-->
