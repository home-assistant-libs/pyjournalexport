# pyjournalexport

Incremental parser for the [systemd journal export format](https://systemd.io/JOURNAL_EXPORT_FORMATS/),
written in Rust with Python bindings.

The export format is what `journalctl -o export` writes and what
`systemd-journal-gatewayd` serves for `Accept: application/vnd.fdo.journal`.
Every entry carries all of its fields, so a consumer that only needs a few
of them spends most of its time skipping the rest. This parser does the
skipping in Rust and only decodes the fields you ask for.

## Usage

```python
from pyjournalexport import JournalExportParser, MalformedBinaryEntryError

parser = JournalExportParser(["__CURSOR", "MESSAGE"])
for chunk in stream:  # bytes, any chunking
    for entry in parser.feed(chunk):  # list of {field: value}
        print(entry["MESSAGE"])
for entry in parser.close():  # trailing entry cut off at EOF
    print(entry["MESSAGE"])
```

- `feed()` returns the entries completed by the data so far; entries without
  any requested field are dropped.
- Binary fields (name, newline, little-endian 64-bit size, data, newline) are
  supported. Values are decoded as UTF-8 with replacement characters.
- The last occurrence of a repeated field wins.
- `MalformedBinaryEntryError` is raised when binary data is not followed by a
  newline, or from `close()` when the stream ended inside a binary field.
- Parsing is linear in the stream length regardless of chunking; an entry
  spanning many feeds is not rescanned.

## Development

Requires a Rust toolchain (https://rustup.rs) and Python 3.12+.

```
uv sync
uv run maturin develop --release
uv run pytest
cargo test
cargo clippy --all-targets -- -D warnings
```

`python benches/bench_parser.py` times the parser on a synthetic stream.

## Release

Tag a release on GitHub; the release workflow builds manylinux and musllinux
wheels for x86_64, aarch64 and armv7, macOS wheels and an sdist, and publishes
them to PyPI via trusted publishing.
