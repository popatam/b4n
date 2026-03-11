from __future__ import annotations

import socket
from dataclasses import dataclass


class AdminCommandError(RuntimeError):
    pass


@dataclass(slots=True)
class NodeAdminClient:
    host: str
    port: int
    timeout_s: float = 2.0

    def send(self, command: str) -> None:
        payload = f"{command.rstrip()}\n".encode("utf-8")

        try:
            with socket.create_connection((self.host, self.port), timeout=self.timeout_s) as sock:
                sock.sendall(payload)
                sock.shutdown(socket.SHUT_WR)
        except OSError as exc:
            raise AdminCommandError(str(exc)) from exc
