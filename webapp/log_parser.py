from __future__ import annotations

import ast
import re

from .models import ParsedSnapshot, ParsedTransaction


class LogParseError(ValueError):
    pass


BLOCKCHAIN_PREFIX = "BlockChain {"
MEMPOOL_PREFIX = "mempool=MemPool {"
SNAPSHOT_MARKER = "BlockChain { chain_id:"

NODE_ID_RE = re.compile(r"\[node (\d+)\]")
BLOCK_INDEX_RE = re.compile(r"\bindex: (\d+)")
TRANSACTION_RE = re.compile(
    r'^Transaction \{ id: (?P<id>\d+), from: (?P<from>\d+), to: (?P<to>\d+), text: (?P<text>"(?:\\.|[^"\\])*") \}$'
)


def count_snapshot_markers(log_text: str) -> int:
    return sum(1 for line in log_text.splitlines() if line.startswith(BLOCKCHAIN_PREFIX))


def parse_latest_snapshot(log_text: str) -> ParsedSnapshot:
    node_id = _parse_node_id(log_text)
    lines = log_text.splitlines()
    parsed_snapshots: list[tuple[tuple[ParsedTransaction, ...], tuple[ParsedTransaction, ...]]] = []

    for blockchain_line, mempool_line in _iter_snapshot_pairs(lines):
        try:
            confirmed = tuple(_parse_blockchain_transactions(blockchain_line))
            pending = tuple(_exclude_confirmed(_parse_mempool_transactions(mempool_line), confirmed))
        except LogParseError:
            continue

        parsed_snapshots.append((confirmed, pending))

    if not parsed_snapshots:
        return ParsedSnapshot(node_id=node_id, confirmed=(), pending=(), snapshot_count=0)

    confirmed, pending = parsed_snapshots[-1]
    return ParsedSnapshot(
        node_id=node_id,
        confirmed=confirmed,
        pending=pending,
        snapshot_count=len(parsed_snapshots),
    )


def _iter_snapshot_pairs(lines: list[str]) -> list[tuple[str, str]]:
    pairs: list[tuple[str, str]] = []

    for idx, line in enumerate(lines):
        if not line.startswith(BLOCKCHAIN_PREFIX):
            continue

        next_lines = lines[idx + 1 : idx + 4]
        mempool_line = next((candidate for candidate in next_lines if candidate.startswith(MEMPOOL_PREFIX)), None)
        if mempool_line is None:
            continue

        pairs.append((line, mempool_line))

    return pairs


def _parse_node_id(log_text: str) -> int | None:
    matches = NODE_ID_RE.findall(log_text)
    if not matches:
        return None

    return int(matches[-1])


def _parse_blockchain_transactions(blockchain_line: str) -> list[ParsedTransaction]:
    blocks_inner = _extract_list(blockchain_line, "blocks: ")
    blocks = _split_top_level_items(blocks_inner)
    confirmed: list[ParsedTransaction] = []

    for block_item in blocks:
        index_match = BLOCK_INDEX_RE.search(block_item)
        if index_match is None:
            raise LogParseError("block index not found")

        block_index = int(index_match.group(1))
        transactions_inner = _extract_list(block_item, "transactions: ")
        transactions = _split_top_level_items(transactions_inner)
        for transaction_item in transactions:
            confirmed.append(_parse_transaction(transaction_item, block_index))

    return confirmed


def _parse_mempool_transactions(mempool_line: str) -> list[ParsedTransaction]:
    queue_inner = _extract_list(mempool_line, "queue: ")
    queue_items = _split_top_level_items(queue_inner)
    return [_parse_transaction(item, None) for item in queue_items]


def _parse_transaction(item: str, block_index: int | None) -> ParsedTransaction:
    match = TRANSACTION_RE.match(item.strip())
    if match is None:
        raise LogParseError("transaction line is malformed")

    return ParsedTransaction(
        tx_id=int(match.group("id")),
        from_id=int(match.group("from")),
        to_id=int(match.group("to")),
        text=_decode_debug_string(match.group("text")),
        block_index=block_index,
    )


def _decode_debug_string(token: str) -> str:
    python_literal = re.sub(
        r"\\u\{([0-9a-fA-F]+)\}",
        lambda match: f"\\U{int(match.group(1), 16):08x}",
        token,
    )
    try:
        return ast.literal_eval(python_literal)
    except (SyntaxError, ValueError) as exc:
        raise LogParseError("string literal is malformed") from exc


def _extract_list(text: str, marker: str) -> str:
    marker_pos = text.find(marker)
    if marker_pos == -1:
        raise LogParseError(f"marker '{marker}' not found")

    open_pos = text.find("[", marker_pos + len(marker))
    if open_pos == -1:
        raise LogParseError("list opening bracket not found")

    close_pos = _find_matching(text, open_pos, "[", "]")
    return text[open_pos + 1 : close_pos]


def _find_matching(text: str, open_pos: int, open_char: str, close_char: str) -> int:
    depth = 0
    in_string = False
    escape = False

    for idx in range(open_pos, len(text)):
        char = text[idx]

        if in_string:
            if escape:
                escape = False
                continue

            if char == "\\":
                escape = True
                continue

            if char == '"':
                in_string = False
            continue

        if char == '"':
            in_string = True
            continue

        if char == open_char:
            depth += 1
            continue

        if char == close_char:
            depth -= 1
            if depth == 0:
                return idx
            continue

    raise LogParseError("matching bracket not found")


def _split_top_level_items(inner: str) -> list[str]:
    if not inner.strip():
        return []

    items: list[str] = []
    start = 0
    brace_depth = 0
    bracket_depth = 0
    in_string = False
    escape = False

    for idx, char in enumerate(inner):
        if in_string:
            if escape:
                escape = False
                continue

            if char == "\\":
                escape = True
                continue

            if char == '"':
                in_string = False
            continue

        if char == '"':
            in_string = True
            continue

        if char == "{":
            brace_depth += 1
            continue

        if char == "}":
            brace_depth -= 1
            continue

        if char == "[":
            bracket_depth += 1
            continue

        if char == "]":
            bracket_depth -= 1
            continue

        if char == "," and brace_depth == 0 and bracket_depth == 0:
            item = inner[start:idx].strip()
            if item:
                items.append(item)
            start = idx + 1

    tail = inner[start:].strip()
    if tail:
        items.append(tail)

    return items


def _exclude_confirmed(
    pending_transactions: tuple[ParsedTransaction, ...] | list[ParsedTransaction],
    confirmed_transactions: tuple[ParsedTransaction, ...] | list[ParsedTransaction],
) -> list[ParsedTransaction]:
    confirmed_keys = {transaction.key for transaction in confirmed_transactions}
    return [transaction for transaction in pending_transactions if transaction.key not in confirmed_keys]
