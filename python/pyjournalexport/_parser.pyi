from collections.abc import Iterable

class MalformedBinaryEntryError(ValueError):
    """A binary field is malformed or the stream ended inside one."""

class JournalExportParser:
    """Incremental parser for the systemd journal export format.

    Feed it the raw stream in any chunking and it returns the completed
    entries as dicts holding only the requested fields.
    """

    def __init__(self, fields: Iterable[str]) -> None: ...
    def feed(self, data: bytes) -> list[dict[str, str]]:
        """Parse data and return the entries completed by it."""

    def close(self) -> list[dict[str, str]]:
        """Return a trailing entry that was cut off at the end of the stream."""
