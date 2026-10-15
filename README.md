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

### Interactive `--ask` Flag

Pass `-a` / `--ask` to any command (`run`, `shell`, `build`, `develop`) to
enable interactive confirmation prompts. Before retrying with `--impure` or
auto-fixing a hash mismatch, `eh` will ask for confirmation:

```bash
eh build nixpkgs#steam --ask
# ? Package 'steam' has an unfree license. Retry with --impure? (Y/n)
# ? Hash mismatch detected in pkgs/hello/default.nix. Fix it? (Y/n)
```

In non-interactive mode (no TTY), `--ask` raises an error rather than silently
skipping. Without `--ask`, prompts only appear in TTY mode.

## Installation

The package installs `eh` and the `nb`, `ns`, `nr`, `nd`, `dev`, `ni`, and `nu`
multicall binaries. Installing `,` is optional.

### NixOS

Add eh to your system flake, import its NixOS module, and enable it. Pass
`inputs` through `specialArgs` so your configuration can refer to the module:

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    eh.url = "github:notashelf/eh";
  };

  outputs = inputs @ {nixpkgs, ...}: {
    nixosConfigurations.myHost = nixpkgs.lib.nixosSystem {
      specialArgs = {inherit inputs;};
      modules = [./configuration.nix];
    };
  };
}
```

In `configuration.nix`, alongside your existing system settings:

```nix
{inputs, ...}: {
  imports = [inputs.eh.nixosModules.default];
  programs.eh.enable = true;
}
```

Remember to replace `myHost` with your system's configuration name.

### Other Nix installations

On non-NixOS systems, install eh in your user profile:

```bash
# Install eh and its multicall binaries.
$ nix profile install github:notashelf/eh
```

You can also try eh without installing it:

```bash
# Show eh's help without adding it to your profile.
$ nix run github:notashelf/eh -- --help
```

### Build from source

From a checkout of this repository, build eh and install it in `~/.local/bin`:

```bash
# Build the release binary.
$ cargo build --release --bin eh

# Install eh, then create the multicall binaries beside it.
$ install -Dm755 target/release/eh "$HOME/.local/bin/eh"
$ cargo xtask multicall --bin-dir "$HOME/.local/bin" \
    --main-binary "$HOME/.local/bin/eh"
```

Put `~/.local/bin` on your `PATH`.

### Optional comma replacement

eh does not install `,` by default. To replace upstream comma with eh's version,
add this setting to your NixOS configuration after enabling eh:

```nix
programs.eh.comma.enable = true;
```

If you install the package without the module, use its `withComma` override:

```nix
environment.systemPackages = [
  (inputs.eh.packages.${pkgs.stdenv.hostPlatform.system}.default.override {
    withComma = true;
  })
];
```

For a source installation, add `--with-comma` to the `cargo xtask multicall`
command above. Remove upstream comma from your installed packages before
enabling this option, since both packages provide `,`.

## Shell Aliases

By default, you may run the `eh` binary akin to Nix with a nicer interface. The
supported Nix commands, i.e., nix `build`, `shell`, `run` and `develop` become
`eh build`, `eh shell`, `eh run` and `eh develop`. You can also invoke a
specific command through the installed multicall binaries. For example, `nb`
will act as `eh build`, `nr` will be `eh run`, and `dev` is an alias for `nd`.

One special example is `eh update`, which is aliased to `nu`, that handles
interactive Nix flake updates. It is special in the sense that the usage is
entirely different from its Nix counterpart, where you get to _interactively_
pick which inputs to update.

With the multicall binaries on your `PATH`, you can use:

```bash
$ ns nixpkgs#hello           # equivalent to: eh shell nixpkgs#hello
$ nr nixpkgs#cowsay "Hello!" # eh run nixpkgs#cowsay
$ nb .#myPackage             # eh build .#myPackage
$ nd .#myPackage             # eh develop .#myPackage
$ dev .#myPackage            # eh develop .#myPackage (same as nd)
$ ni nixpkgs#hello           # eh info nixpkgs#hello
$ nu                         # interactive flake input updates
```

### Execute a binary

`eh exec` finds a package containing the binary and runs it through Nix:

```bash
# Find and run glxgears.
$ eh exec glxgears
# Pass --help to hello, with confirmation before retries or fixes.
$ eh exec --ask hello --help
```

Put eh options before the binary name. Everything after it goes to the program,
including `--help`, `-v`, and `--`. Use `--ask` to ask before retries and hash
fixes. `eh exec` reads the `exec` settings in your eh config.

### Locate files

`eh locate` searches file paths in the SPAM index. Patterns are literal unless
you pass `--regex`:

```bash
# Find executable files named hello.
$ eh locate /bin/hello --whole-name --type x
# Search with a regular expression.
$ eh locate 'bin/(hello|cowsay)$' --regex
# Print matching package names only.
$ eh locate hello --package hello --minimal
```

- `-r` / `--regex` treats path and package patterns as regular expressions.
- `-p` / `--package` filters package names.
- `-t` / `--type` filters by file type. Use `r` for regular files, `x` for
  executables, `d` for directories, or `s` for symlinks. Repeat it to include
  more than one type.
- `--at-root` matches from the start of the output-relative path.
- `-w` / `--whole-name` requires the match to end at the end of the path.
- `--minimal` prints each matching package name once.
- `--no-group` includes paths below a matched directory.
- `--db FILE` reads a different database file.

### SPAM database

`eh exec`, `eh locate`, and `,` need a SPAM package index. They cannot read a
nix-index database. Set `SPAM_DATABASE` to the database file, or put it at
`$XDG_CACHE_HOME/spam/spam.db`. Without `XDG_CACHE_HOME`, the default is
`~/.cache/spam/spam.db`.

To generate an index:

```bash
# Generating an index is RAM-intensive.
$ spam index --nixpkgs '<nixpkgs>' --scope pkg --output ~/.cache/spam/spam.db
```

### Comma compatibility

Once you enable comma support, use `,` as you would upstream comma:

```bash
# Run a binary by name.
$ , glxgears
# Ask before running hello and pass it --help.
$ , --ask hello --help
# List providers or print the executable path.
$ , --print-packages hello
$ , --print-path hello
```

`,` accepts upstream comma's options, including `--install`, `--shell`,
`--picker`, `--nixpkgs-flake`, `--ask`, `--print-packages`, `--print-path`,
`--cache-level`, `--empty-cache`, and `--delete-entry`. It also has `man` and
completion commands. Run `, --help` for the full list.

It reads these environment variables instead of eh's config:

- `COMMA_PICKER` selects the package picker. The default is `fzy`.
- `COMMA_NIXPKGS_FLAKE` selects the Nixpkgs flake. The default is `nixpkgs`.
- `COMMA_ASK_TO_CONFIRM` asks before running the program.
- `COMMA_CACHING` sets the cache level, like `--cache-level`. The default is
  `2`.

Cache level `0` disables caching. Level `1` remembers package choices. Level `2`
also remembers executable paths and rebuilds if garbage collection removed them.
The cache lives at `$XDG_CACHE_HOME/eh/comma/choices-v1.json`, or under
`~/.cache` when `XDG_CACHE_HOME` is unset. It does not read upstream comma's
cache.

Like `eh exec`, `,` uses SPAM for package lookup and passes arguments after the
binary name to the program unchanged.

### Shell Completions

Generate completions for bash, zsh, or fish:

```bash
# Generate completions for your shell
$ eh completion bash
$ eh completion zsh
$ eh completion fish
```

For `,` completions:

```bash
# Print completions for your shell.
$ , --print-completions bash
$ , --print-completions zsh
$ , --print-completions fish
```

For a local install, generate completion files with:

```bash
# Write completion files for eh and the comma replacement.
$ cargo xtask completions bash --main-binary "$HOME/.local/bin/eh" --with-comma
```

Repeat for `zsh` or `fish`, then install the files from `completions/` in your
shell's completion directory. Install `,` beside `eh` before running this
command. Omit `--with-comma` if you only want eh's completions. Nix packages
install the completion files for you.

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

> [!TIP]
> When `impure` is absent (the default), auto-retry with `--impure` is
> **automatic**. `eh` will add `--impure` and the appropriate `NIXPKGS_ALLOW_*`
> variable whenever it detects an unfree, insecure, or broken package.

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

[provided here]: https://joinup.ec.europa.eu/collection/eupl/eupl-text-eupl-12

This project is made available under the European Union Public Licence (EUPL)
version 1.2. See [LICENSE](LICENSE) for more details on the exact conditions. An
online copy is [provided here].
