from __future__ import annotations

from dataclasses import dataclass
from typing import Any


@dataclass(frozen=True, slots=True)
class ParsedTransaction:
    tx_id: int
    from_id: int
    to_id: int
    text: str
    block_index: int | None = None

    @property
    def key(self) -> str:
        return f"{self.tx_id}:{self.from_id}:{self.to_id}:{self.text}"

    def to_message(self, status: str) -> dict[str, Any]:
        return {
            "key": self.key,
            "id": self.tx_id,
            "from": self.from_id,
            "to": self.to_id,
            "text": self.text,
            "status": status,
            "block_index": self.block_index,
        }


@dataclass(frozen=True, slots=True)
class ParsedSnapshot:
    node_id: int | None
    confirmed: tuple[ParsedTransaction, ...]
    pending: tuple[ParsedTransaction, ...]
    snapshot_count: int
