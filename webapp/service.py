from __future__ import annotations

import threading
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from .log_parser import count_snapshot_markers, parse_latest_snapshot
from .node_admin import AdminCommandError, NodeAdminClient


class SnapshotUnavailableError(RuntimeError):
    pass


@dataclass(slots=True)
class NodeChatService:
    admin_client: NodeAdminClient
    log_path: Path
    refresh_timeout_s: float = 2.0
    refresh_interval_s: float = 0.1
    _lock: threading.Lock = field(default_factory=threading.Lock, init=False, repr=False)

    def get_chat_state(self, force_refresh: bool = True) -> dict[str, Any]:
        with self._lock:
            snapshot = self._load_snapshot_locked(force_refresh=force_refresh)
            return self._serialize_snapshot(snapshot)

    def send_message(self, text: str) -> dict[str, Any]:
        cleaned = text.strip()
        if not cleaned:
            raise ValueError("message text must not be empty")
        if "\n" in cleaned or "\r" in cleaned:
            raise ValueError("message text must be a single line")

        with self._lock:
            self.admin_client.send(f"trx {cleaned}")
            snapshot = self._load_snapshot_locked(force_refresh=True)
            return self._serialize_snapshot(snapshot)

    def _load_snapshot_locked(self, force_refresh: bool) -> Any:
        before_log = self._read_log()
        before_snapshot_count = count_snapshot_markers(before_log)

        if force_refresh:
            self.admin_client.send("print")
            deadline = time.monotonic() + self.refresh_timeout_s

            while time.monotonic() < deadline:
                current_log = self._read_log()
                if count_snapshot_markers(current_log) > before_snapshot_count:
                    snapshot = parse_latest_snapshot(current_log)
                    if snapshot.snapshot_count > 0:
                        return snapshot
                time.sleep(self.refresh_interval_s)

        snapshot = parse_latest_snapshot(self._read_log())
        if snapshot.snapshot_count == 0:
            raise SnapshotUnavailableError(f"No complete blockchain snapshot found in {self.log_path}")

        return snapshot

    def _read_log(self) -> str:
        try:
            return self.log_path.read_text(encoding="utf-8", errors="replace")
        except FileNotFoundError:
            return ""

    @staticmethod
    def _serialize_snapshot(snapshot: Any) -> dict[str, Any]:
        confirmed_messages = [transaction.to_message("confirmed") for transaction in snapshot.confirmed]
        pending_messages = [transaction.to_message("pending") for transaction in snapshot.pending]

        return {
            "node_id": snapshot.node_id,
            "messages": confirmed_messages + pending_messages,
            "totals": {
                "confirmed": len(confirmed_messages),
                "pending": len(pending_messages),
            },
        }


def parse_host_port(address: str) -> tuple[str, int]:
    host, port_text = address.rsplit(":", 1)
    return host, int(port_text)


def build_service(
    admin_address: str,
    log_path: str,
    refresh_timeout_s: float = 2.0,
    refresh_interval_s: float = 0.1,
) -> NodeChatService:
    host, port = parse_host_port(admin_address)
    return NodeChatService(
        admin_client=NodeAdminClient(host=host, port=port, timeout_s=refresh_timeout_s),
        log_path=Path(log_path),
        refresh_timeout_s=refresh_timeout_s,
        refresh_interval_s=refresh_interval_s,
    )


def wrap_service_error(exc: Exception) -> tuple[int, str]:
    if isinstance(exc, ValueError):
        return 400, str(exc)
    if isinstance(exc, AdminCommandError):
        return 503, f"Node admin socket is unavailable: {exc}"
    if isinstance(exc, SnapshotUnavailableError):
        return 503, str(exc)
    return 500, "Unexpected server error"
