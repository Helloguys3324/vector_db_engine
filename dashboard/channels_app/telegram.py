import hashlib
import hmac
import json
import time
from dataclasses import dataclass
from urllib.error import URLError
from urllib.parse import urlencode
from urllib.request import urlopen

from django.conf import settings

TELEGRAM_LOGIN_MAX_AGE_SECONDS = 86_400
ADMIN_STATUSES = {'administrator', 'creator'}
_BOT_USERNAME_CACHE: str | None = None


@dataclass(frozen=True)
class TelegramSessionUser:
    id: int
    first_name: str
    username: str
    last_name: str = ''
    photo_url: str = ''


def _telegram_secret_key(bot_token: str) -> bytes:
    return hashlib.sha256(bot_token.encode('utf-8')).digest()


def validate_login_payload(raw_payload: dict[str, str]) -> tuple[bool, str]:
    bot_token = settings.DASHBOARD_TELEGRAM_BOT_TOKEN
    if not bot_token:
        return False, 'Telegram auth is not configured: DASHBOARD_TELEGRAM_BOT_TOKEN is missing.'

    payload = {k: v for k, v in raw_payload.items() if v is not None}
    auth_hash = payload.pop('hash', None)
    if not auth_hash:
        return False, 'Telegram auth hash is missing.'

    if 'auth_date' not in payload:
        return False, 'Telegram auth_date is missing.'

    try:
        auth_date = int(payload['auth_date'])
    except ValueError:
        return False, 'Telegram auth_date is invalid.'

    if time.time() - auth_date > TELEGRAM_LOGIN_MAX_AGE_SECONDS:
        return False, 'Telegram login request is too old.'

    data_check_string = '\n'.join(f'{k}={payload[k]}' for k in sorted(payload))
    computed = hmac.new(
        _telegram_secret_key(bot_token),
        msg=data_check_string.encode('utf-8'),
        digestmod=hashlib.sha256,
    ).hexdigest()
    if not hmac.compare_digest(computed, auth_hash):
        return False, 'Telegram auth signature mismatch.'

    return True, ''


def session_user_from_payload(payload: dict[str, str]) -> TelegramSessionUser:
    return TelegramSessionUser(
        id=int(payload['id']),
        first_name=payload.get('first_name', ''),
        username=payload.get('username', ''),
        last_name=payload.get('last_name', ''),
        photo_url=payload.get('photo_url', ''),
    )


def resolve_telegram_login_bot_username() -> str:
    global _BOT_USERNAME_CACHE

    configured_username = settings.DASHBOARD_TELEGRAM_BOT_USERNAME.strip()
    if configured_username:
        return configured_username

    if _BOT_USERNAME_CACHE is not None:
        return _BOT_USERNAME_CACHE

    bot_token = settings.DASHBOARD_TELEGRAM_BOT_TOKEN
    if not bot_token:
        return ''

    endpoint = f'https://api.telegram.org/bot{bot_token}/getMe'
    try:
        with urlopen(endpoint, timeout=8) as response:
            payload = json.loads(response.read().decode('utf-8'))
    except URLError as exc:
        raise RuntimeError(f'Failed to resolve Telegram bot username via getMe: {exc}') from exc

    if not payload.get('ok'):
        raise RuntimeError('Telegram getMe failed while resolving dashboard login bot username.')

    username = (payload.get('result') or {}).get('username') or ''
    if not username:
        raise RuntimeError('Telegram getMe returned empty username for dashboard login.')

    _BOT_USERNAME_CACHE = username
    return username


def get_chat_member_status(chat_id: str, user_id: int) -> str | None:
    bot_token = settings.DASHBOARD_TELEGRAM_BOT_TOKEN
    if not bot_token:
        raise RuntimeError('Telegram bot token is not configured for dashboard.')

    endpoint = f'https://api.telegram.org/bot{bot_token}/getChatMember'
    query = urlencode({'chat_id': chat_id, 'user_id': user_id})
    try:
        with urlopen(f'{endpoint}?{query}', timeout=8) as response:
            payload = json.loads(response.read().decode('utf-8'))
    except URLError as exc:
        raise RuntimeError(f'Telegram API request failed for chat {chat_id}: {exc}') from exc

    if not payload.get('ok'):
        return None

    result = payload.get('result') or {}
    return result.get('status')


def is_user_chat_admin(chat_id: str, user_id: int) -> bool:
    return get_chat_member_status(chat_id, user_id) in ADMIN_STATUSES
