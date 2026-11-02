"""Time the parser on a synthetic export stream resembling Docker journald entries."""

import sys
import time

from pyjournalexport import JournalExportParser

N = int(sys.argv[1]) if len(sys.argv) > 1 else 50_000
CHUNK = 64 * 1024
FIELDS = {
    "plain": ["__CURSOR", "MESSAGE"],
    "verbose": [
        "__CURSOR",
        "__REALTIME_TIMESTAMP",
        "_HOSTNAME",
        "SYSLOG_IDENTIFIER",
        "_PID",
        "MESSAGE",
    ],
}


def entry(i: int) -> bytes:
    """Build one export entry with the ~30 fields Docker's journald driver adds."""
    fields = {
        "__CURSOR": f"s=0123456789abcdef0123456789abcdef;i={i:x};b=fedcba9876543210fedcba9876543210;m={i * 1000:x};t=6{i:013x};x={i * 7:016x}",
        "__REALTIME_TIMESTAMP": str(1761700000000000 + i * 1000),
        "__MONOTONIC_TIMESTAMP": str(100000000 + i * 1000),
        "__SEQNUM": str(i),
        "__SEQNUM_ID": "0123456789abcdef0123456789abcdef",
        "_BOOT_ID": "fedcba9876543210fedcba9876543210",
        "_MACHINE_ID": "00112233445566778899aabbccddeeff",
        "_HOSTNAME": "homeassistant",
        "_TRANSPORT": "journal",
        "PRIORITY": "6",
        "_UID": "0",
        "_GID": "0",
        "_CAP_EFFECTIVE": "1ffffffffff",
        "_SYSTEMD_SLICE": "system.slice",
        "_COMM": "dockerd",
        "_EXE": "/usr/bin/dockerd",
        "_CMDLINE": "/usr/bin/dockerd -H fd:// --log-driver=journald --data-root /mnt/data/docker",
        "_SYSTEMD_CGROUP": "/system.slice/docker.service",
        "_SYSTEMD_UNIT": "docker.service",
        "_SYSTEMD_INVOCATION_ID": "aabbccddeeff00112233445566778899",
        "_PID": "512",
        "CONTAINER_NAME": "homeassistant",
        "CONTAINER_ID": "a1b2c3d4e5f6",
        "CONTAINER_ID_FULL": "a1b2c3d4e5f6" * 5 + "a1b2",
        "CONTAINER_TAG": "a1b2c3d4e5f6",
        "IMAGE_NAME": "ghcr.io/home-assistant/qemux86-64-homeassistant:2026.10.4",
        "SYSLOG_IDENTIFIER": "homeassistant",
        "CONTAINER_LOG_EPOCH": "0123456789abcdef0123456789abcdef",
        "CONTAINER_LOG_ORDINAL": str(i),
        "_SOURCE_REALTIME_TIMESTAMP": str(1761700000000000 + i * 1000 - 5),
        "_RUNTIME_SCOPE": "system",
        "MESSAGE": f"\x1b[32m2026-10-29 10:00:00.{i % 1000:03d} INFO (MainThread) [homeassistant.components.sensor] Setting up sensor.some_entity_{i}\x1b[0m",
    }
    return b"".join(f"{k}={v}\n".encode() for k, v in fields.items()) + b"\n"


def main() -> None:
    """Run the benchmark."""
    payload = b"".join(entry(i) for i in range(N))
    print(f"{N} entries, {len(payload) / 1e6:.1f} MB, {CHUNK // 1024} KiB feeds")
    for label, fields in FIELDS.items():
        best = float("inf")
        for _ in range(3):
            parser = JournalExportParser(fields)
            count = 0
            start = time.perf_counter()
            for i in range(0, len(payload), CHUNK):
                count += len(parser.feed(payload[i : i + CHUNK]))
            count += len(parser.close())
            best = min(best, time.perf_counter() - start)
        print(f"{label:8s} {best:.3f}s ({count} entries)")


if __name__ == "__main__":
    main()
