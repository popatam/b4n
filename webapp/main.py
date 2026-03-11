from __future__ import annotations

import os
from pathlib import Path

from fastapi import FastAPI, HTTPException, Request
from fastapi.responses import HTMLResponse
from fastapi.staticfiles import StaticFiles
from fastapi.templating import Jinja2Templates
from pydantic import BaseModel, Field

from .service import build_service, wrap_service_error


BASE_DIR = Path(__file__).resolve().parent
templates = Jinja2Templates(directory=str(BASE_DIR / "templates"))

app = FastAPI(title="B4 Chat UI")
app.mount("/static", StaticFiles(directory=str(BASE_DIR / "static")), name="static")

service = build_service(
    admin_address=os.getenv("B4_ADMIN_TARGET", os.getenv("B4_ADMIN", "127.0.0.1:17001")),
    log_path=os.getenv("B4_LOG_PATH", "/var/log/b4/node.log"),
    refresh_timeout_s=float(os.getenv("B4_REFRESH_TIMEOUT_S", "2.0")),
    refresh_interval_s=float(os.getenv("B4_REFRESH_INTERVAL_S", "0.1")),
)


class MessagePayload(BaseModel):
    text: str = Field(min_length=1, max_length=500)


@app.get("/", response_class=HTMLResponse)
async def index(request: Request) -> HTMLResponse:
    return templates.TemplateResponse(
        request=request,
        name="index.html",
        context={
            "request": request,
            "poll_interval_ms": int(os.getenv("B4_POLL_INTERVAL_MS", "2000")),
        },
    )


@app.get("/api/messages")
async def messages() -> dict:
    try:
        return service.get_chat_state(force_refresh=True)
    except Exception as exc:
        status_code, detail = wrap_service_error(exc)
        raise HTTPException(status_code=status_code, detail=detail) from exc


@app.post("/api/messages")
async def send_message(payload: MessagePayload) -> dict:
    try:
        return service.send_message(payload.text)
    except Exception as exc:
        status_code, detail = wrap_service_error(exc)
        raise HTTPException(status_code=status_code, detail=detail) from exc


@app.get("/healthz")
async def healthz() -> dict[str, str]:
    return {"status": "ok"}
