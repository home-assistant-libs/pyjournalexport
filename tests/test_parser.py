"""Tests for the Python interface of pyjournalexport."""

import pytest
from pyjournalexport import JournalExportParser, MalformedBinaryEntryError

EXPORT = (
    b"__CURSOR=cursor1\n"
    b"MESSAGE\n\x0e\x00\x00\x00\x00\x00\x00\x00Hello,\nworld!\n\n"
    b"AFTER=after\n\n"
    b"__CURSOR=cursor2\n"
    b"MESSAGE=Hello again, world!\n\n"
    b"MESSAGE=No cursor\n"
)


@pytest.mark.parametrize("chunk_size", [1, 7, len(EXPORT)])
def test_split_feeds(chunk_size: int) -> None:
    """Entries and binary fields split across feeds are parsed."""
    parser = JournalExportParser(["__CURSOR", "MESSAGE"])
    entries = []
    for i in range(0, len(EXPORT), chunk_size):
        entries += parser.feed(EXPORT[i : i + chunk_size])
    entries += parser.close()

    assert entries == [
        {"__CURSOR": "cursor1", "MESSAGE": "Hello,\nworld!\n"},
        {"__CURSOR": "cursor2", "MESSAGE": "Hello again, world!"},
        {"MESSAGE": "No cursor"},
    ]


def test_skips_unwanted_fields() -> None:
    """Only requested fields are returned, including from binary fields."""
    parser = JournalExportParser(["MESSAGE"])
    assert parser.feed(
        b"ID=1\nOTHER\n\x02\x00\x00\x00\x00\x00\x00\x00\n\n\nMESSAGE=Hi\n\n"
    ) == [{"MESSAGE": "Hi"}]
    assert not parser.close()


def test_non_utf8_replaced() -> None:
    """Invalid UTF-8 is replaced in simple and binary fields."""
    parser = JournalExportParser(["MESSAGE"])
    assert parser.feed(b"MESSAGE=Hello, \xff world!\n\n") == [
        {"MESSAGE": "Hello, � world!"}
    ]
    assert parser.feed(
        b"MESSAGE\n\x0f\x00\x00\x00\x00\x00\x00\x00Hello, \xff world!\n\n"
    ) == [{"MESSAGE": "Hello, � world!"}]


def test_malformed_binary_separator() -> None:
    """Binary data not followed by a newline raises."""
    parser = JournalExportParser(["MESSAGE"])
    with pytest.raises(MalformedBinaryEntryError):
        parser.feed(
            b"ID=1\nMESSAGE\n\x0d\x00\x00\x00\x00\x00\x00\x00Hello, world!AFTER=after\n\n"
        )


@pytest.mark.parametrize(
    "data",
    [
        pytest.param(b"MESSAGE", id="in-name"),
        pytest.param(b"MESSAGE\n\x0d\x00\x00\x00", id="in-size"),
        pytest.param(b"MESSAGE\n\x0d\x00\x00\x00\x00\x00\x00\x00Hello", id="in-data"),
        pytest.param(
            b"MESSAGE\n\x05\x00\x00\x00\x00\x00\x00\x00Hello", id="missing-separator"
        ),
    ],
)
def test_truncated_binary_field(data: bytes) -> None:
    """A stream ending inside a binary field raises on close."""
    parser = JournalExportParser(["MESSAGE"])
    assert not parser.feed(data)
    with pytest.raises(MalformedBinaryEntryError):
        parser.close()


@pytest.mark.parametrize(
    "data",
    [
        pytest.param(b"MESSAGE=first\nMESSAGE=last\n\n", id="simple"),
        pytest.param(
            b"MESSAGE=first\nBIN\n\x01\x00\x00\x00\x00\x00\x00\x00\n\nMESSAGE=last\n\n",
            id="binary",
        ),
    ],
)
def test_repeated_field_last_wins(data: bytes) -> None:
    """The last occurrence of a repeated field is used."""
    parser = JournalExportParser(["MESSAGE"])
    assert parser.feed(data) == [{"MESSAGE": "last"}]


def test_binary_field_split_after_completed_entry() -> None:
    """A binary field split across feeds is parsed after buffer compaction."""
    parser = JournalExportParser(["MESSAGE"])
    payload = b"a\n\nbcdefghijk"
    first = (
        b"MESSAGE=" + b"x" * 40 + b"\n\n"
        b"MESSAGE\n" + len(payload).to_bytes(8, "little") + payload[:6]
    )
    assert parser.feed(first) == [{"MESSAGE": "x" * 40}]
    assert parser.feed(payload[6:] + b"\n\n") == [{"MESSAGE": payload.decode()}]
    assert not parser.close()


def test_large_binary_field_in_small_feeds() -> None:
    """A binary field spanning many feeds is parsed."""
    parser = JournalExportParser(["MESSAGE"])
    payload = b"x" * (8 * 1024 * 1024)
    data = (
        b"MESSAGE\n" + len(payload).to_bytes(8, "little") + payload + b"\n\nAFTER=1\n\n"
    )
    entries = []
    for i in range(0, len(data), 4096):
        entries += parser.feed(data[i : i + 4096])
    assert entries == [{"MESSAGE": payload.decode()}]
    assert parser.feed(b"MESSAGE=next\n\n") == [{"MESSAGE": "next"}]


def test_long_text_field_in_small_feeds() -> None:
    """A text field spanning many feeds is parsed without rescanning."""
    parser = JournalExportParser(["MESSAGE"])
    assert parser.feed(b"MESSAGE=") == []
    for _ in range(10_000):
        assert parser.feed(b"x" * 1000) == []
    assert parser.feed(b"\n\n") == [{"MESSAGE": "x" * 10_000_000}]
