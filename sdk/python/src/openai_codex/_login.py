from __future__ import annotations

from dataclasses import dataclass
from typing import Protocol

from .async_client import AsyncCodexClient
from .client import CodexClient
from .generated.v2_all import (
    AccountLoginCompletedNotification,
    CancelLoginAccountResponse,
)

_ASTRAL_CHATGPT_LOGIN_DISABLED = (
    "ChatGPT login is not available in astral-code. Configure a provider-neutral "
    "API key and base URL instead, for example ASTRAL_API_KEY and ASTRAL_BASE_URL."
)


class _AsyncLoginOwner(Protocol):
    """Subset of AsyncCodex needed by async login handles."""

    _client: AsyncCodexClient

    async def _ensure_initialized(self) -> None:
        """Ensure the owning SDK client has a live Codex connection."""
        ...


def start_chatgpt_login(client: CodexClient) -> ChatgptLoginHandle:
    """ChatGPT browser login is disabled in astral-code."""
    raise RuntimeError(_ASTRAL_CHATGPT_LOGIN_DISABLED)


async def async_start_chatgpt_login(owner: _AsyncLoginOwner) -> AsyncChatgptLoginHandle:
    """ChatGPT browser login is disabled in astral-code."""
    raise RuntimeError(_ASTRAL_CHATGPT_LOGIN_DISABLED)


def start_device_code_login(client: CodexClient) -> DeviceCodeLoginHandle:
    """ChatGPT device-code login is disabled in astral-code."""
    raise RuntimeError(_ASTRAL_CHATGPT_LOGIN_DISABLED)


async def async_start_device_code_login(
    owner: _AsyncLoginOwner,
) -> AsyncDeviceCodeLoginHandle:
    """ChatGPT device-code login is disabled in astral-code."""
    raise RuntimeError(_ASTRAL_CHATGPT_LOGIN_DISABLED)


@dataclass(slots=True)
class ChatgptLoginHandle:
    """Live browser-login attempt returned by `Codex.login_chatgpt()`."""

    _client: CodexClient
    login_id: str
    auth_url: str

    def wait(self) -> AccountLoginCompletedNotification:
        """Wait for this browser login attempt's completion notification."""
        return self._client.wait_for_login_completed(self.login_id)

    def cancel(self) -> CancelLoginAccountResponse:
        """Cancel this browser login attempt."""
        return self._client.account_login_cancel(self.login_id)


@dataclass(slots=True)
class DeviceCodeLoginHandle:
    """Live device-code login attempt returned by `Codex.login_chatgpt_device_code()`."""

    _client: CodexClient
    login_id: str
    verification_url: str
    user_code: str

    def wait(self) -> AccountLoginCompletedNotification:
        """Wait for this device-code login attempt's completion notification."""
        return self._client.wait_for_login_completed(self.login_id)

    def cancel(self) -> CancelLoginAccountResponse:
        """Cancel this device-code login attempt."""
        return self._client.account_login_cancel(self.login_id)


@dataclass(slots=True)
class AsyncChatgptLoginHandle:
    """Live browser-login attempt returned by `AsyncCodex.login_chatgpt()`."""

    _codex: _AsyncLoginOwner
    login_id: str
    auth_url: str

    async def wait(self) -> AccountLoginCompletedNotification:
        """Wait for this browser login attempt's completion notification."""
        await self._codex._ensure_initialized()
        return await self._codex._client.wait_for_login_completed(self.login_id)

    async def cancel(self) -> CancelLoginAccountResponse:
        """Cancel this browser login attempt."""
        await self._codex._ensure_initialized()
        return await self._codex._client.account_login_cancel(self.login_id)


@dataclass(slots=True)
class AsyncDeviceCodeLoginHandle:
    """Live device-code attempt returned by `AsyncCodex.login_chatgpt_device_code()`."""

    _codex: _AsyncLoginOwner
    login_id: str
    verification_url: str
    user_code: str

    async def wait(self) -> AccountLoginCompletedNotification:
        """Wait for this device-code login attempt's completion notification."""
        await self._codex._ensure_initialized()
        return await self._codex._client.wait_for_login_completed(self.login_id)

    async def cancel(self) -> CancelLoginAccountResponse:
        """Cancel this device-code login attempt."""
        await self._codex._ensure_initialized()
        return await self._codex._client.account_login_cancel(self.login_id)
