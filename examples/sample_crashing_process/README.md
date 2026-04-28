# Sample Crashing Process Example (Linux Only)

This example demonstrates how the `module_info` crate embeds metadata into ELF binaries and how this metadata is preserved when a program crashes, making it available in core dumps.

> **Important**: This example is specifically designed for Linux systems and will not work on other platforms. It relies on Linux-specific ELF binary formats and signal handling.

## Purpose

The main goals of this example are:

1. Show how `module_info` embeds vital metadata into the binary
2. Demonstrate various crash scenarios that produce core dumps containing this metadata
3. Provide a tool for testing crash handling in Linux environments

## Platform Support

This example is **Linux-only**. It won't compile or run correctly on other platforms because:

1. It uses Linux-specific ELF binary format features
2. It relies on the `nix` crate for POSIX signal handling
3. It demonstrates features that specifically interact with Linux core dumps

## Building the Example

```bash
# Navigate to the example directory
cd examples/sample_crashing_process

# Build the example
cargo build

# Run the example with a specific crash type
cargo run -- SIGABRT
```

## Available Commands

The following crash types can be triggered. All of them produce a core
dump on a default Linux configuration *except* `SIGTERM`, which exits
without dumping unless you change the kernel's default disposition.

- `SIGABRT` - Abort signal (e.g., from `abort()`)
- `SIGFPE` - Floating-point exception
- `SIGILL` - Illegal instruction
- `SIGINT` - Interrupt signal
- `SIGSEGV` - Segmentation violation (memory access error)
- `SIGTERM` - Termination request (no core dump by default)
- `EXCEPTION` - Generic unhandled exception
- `INFO` - Display module info metadata without crashing

## Viewing Embedded Metadata

To examine the metadata embedded in the binary:

```bash
# View the .note.package section as printable strings (the embedded JSON
# falls out directly):
readelf -p .note.package ./target/debug/sample_crashing_process

# Or dump the raw section without readelf's caret-encoded newlines:
objcopy --dump-section .note.package=/dev/stdout ./target/debug/sample_crashing_process

# Get metadata from the running program:
cargo run -- INFO
```

## Core Dump Analysis

When the program crashes (except for SIGTERM), it produces a core dump if core dumps are enabled on your system. To enable core dumps:

```bash
# Set core dump size limit (unlimited)
ulimit -c unlimited

# Configure core dump pattern (optional)
sudo sysctl -w kernel.core_pattern=/tmp/core-%e.%p
```

After a crash, you can extract the embedded metadata directly from the
core dump. The `.note.package` section travels with the dump, so no
matching binary on disk is required:

```bash
# Pull the metadata out of the dump:
readelf -p .note.package /tmp/core-sample_crashing_process.12345
```

On systemd >= 248, `systemd-coredump` parses `.note.package` automatically
and indexes it into the journal. `coredumpctl info <PID>` will then show
fields like `COREDUMP_PACKAGE_NAME`, `COREDUMP_PACKAGE_VERSION`, and
`COREDUMP_PACKAGE_JSON` without you having to invoke `readelf` at all.
WinDbg also reads the section natively when opening Linux core dumps.

GDB itself does not parse `.note.package`, so loading the dump with
`gdb ./binary core.dump` will not display the module-info fields; use
`readelf` (or `coredumpctl`) for that.

This demonstrates how the module metadata is preserved in the core dump, making it valuable for post-mortem debugging and crash analysis.

## Integration with Crash Reporting Systems

In real-world applications, the metadata embedded by `module_info` helps crash reporting systems identify:

- Package Version and exact version of the binary (module version)
- Git repo, branch and commit hash
- Maintainer contact
- Other valuable context for debugging

This example can be used to generate test crashes for crash reporting systems that extract this metadata.
