from functools import wraps

from django.conf import settings
from django.contrib import messages
from django.core.exceptions import PermissionDenied
from django.shortcuts import get_object_or_404, redirect, render

from .forms import ManagedChannelForm
from .models import ManagedChannel
from .telegram import (
    TELEGRAM_LOGIN_MAX_AGE_SECONDS,
    is_user_chat_admin,
    resolve_telegram_login_bot_username,
    session_user_from_payload,
    validate_login_payload,
)

SESSION_TELEGRAM_USER_KEY = 'telegram_user'


def telegram_login_required(view_func):
    @wraps(view_func)
    def _wrapped(request, *args, **kwargs):
        if SESSION_TELEGRAM_USER_KEY not in request.session:
            return redirect('channels:login')
        return view_func(request, *args, **kwargs)

    return _wrapped


def _current_telegram_user_id(request) -> int:
    telegram_user = request.session.get(SESSION_TELEGRAM_USER_KEY)
    if not telegram_user or 'id' not in telegram_user:
        raise PermissionDenied('Telegram session is missing.')
    return int(telegram_user['id'])


def _assert_chat_admin_or_forbidden(channel: ManagedChannel, telegram_user_id: int) -> None:
    if not is_user_chat_admin(channel.external_id, telegram_user_id):
        raise PermissionDenied('You do not have admin rights in this Telegram chat.')


def login_page(request):
    if request.session.get(SESSION_TELEGRAM_USER_KEY):
        return redirect('channels:home')
    telegram_login_bot_username = ''
    try:
        telegram_login_bot_username = resolve_telegram_login_bot_username()
    except RuntimeError as exc:
        messages.error(request, str(exc))

    return render(
        request,
        'dashboard/login.html',
        {
            'telegram_login_bot_username': telegram_login_bot_username,
            'telegram_login_ready': bool(
                settings.DASHBOARD_TELEGRAM_BOT_TOKEN and telegram_login_bot_username
            ),
        },
    )


def telegram_auth_callback(request):
    payload = {key: value for key, value in request.GET.items()}
    is_valid, reason = validate_login_payload(payload)
    if not is_valid:
        messages.error(request, reason)
        return redirect('channels:login')

    telegram_user = session_user_from_payload(payload)
    request.session[SESSION_TELEGRAM_USER_KEY] = {
        'id': telegram_user.id,
        'first_name': telegram_user.first_name,
        'last_name': telegram_user.last_name,
        'username': telegram_user.username,
        'photo_url': telegram_user.photo_url,
    }
    request.session.set_expiry(TELEGRAM_LOGIN_MAX_AGE_SECONDS)
    return redirect('channels:home')


@telegram_login_required
def logout_page(request):
    request.session.pop(SESSION_TELEGRAM_USER_KEY, None)
    return redirect('channels:login')


@telegram_login_required
def dashboard_home(request):
    telegram_user_id = _current_telegram_user_id(request)
    all_channels = ManagedChannel.objects.filter(platform=ManagedChannel.Platform.TELEGRAM)
    channels = []
    for channel in all_channels:
        try:
            if is_user_chat_admin(channel.external_id, telegram_user_id):
                channels.append(channel)
        except RuntimeError as exc:
            messages.error(request, str(exc))
            break

    total_scanned = sum(channel.messages_scanned for channel in channels)
    total_blocked = sum(channel.blocked_messages for channel in channels)
    total_channels = len(channels)
    blocked_percent = (total_blocked / total_scanned * 100) if total_scanned else 0

    return render(
        request,
        'dashboard/home.html',
        {
            'channels': channels,
            'total_channels': total_channels,
            'total_scanned': total_scanned,
            'total_blocked': total_blocked,
            'blocked_percent': blocked_percent,
        },
    )


@telegram_login_required
def channel_create(request):
    telegram_user_id = _current_telegram_user_id(request)
    if request.method == 'POST':
        form = ManagedChannelForm(request.POST)
        if form.is_valid():
            external_id = form.cleaned_data['external_id']
            try:
                if not is_user_chat_admin(external_id, telegram_user_id):
                    form.add_error(
                        'external_id',
                        'You are not an admin in this Telegram chat.',
                    )
            except RuntimeError as exc:
                form.add_error('external_id', str(exc))
        if form.is_valid():
            channel = form.save()
            return redirect('channels:channel-detail', pk=channel.pk)
    else:
        form = ManagedChannelForm()
    return render(
        request,
        'dashboard/channel_form.html',
        {
            'form': form,
        },
    )


@telegram_login_required
def channel_detail(request, pk):
    channel = get_object_or_404(ManagedChannel, pk=pk)
    if channel.platform != ManagedChannel.Platform.TELEGRAM:
        raise PermissionDenied('Only Telegram channels are supported in this dashboard.')

    telegram_user_id = _current_telegram_user_id(request)
    try:
        _assert_chat_admin_or_forbidden(channel, telegram_user_id)
    except RuntimeError as exc:
        messages.error(request, str(exc))
        return redirect('channels:home')

    if request.method == 'POST':
        form = ManagedChannelForm(request.POST, instance=channel)
        if form.is_valid():
            external_id = form.cleaned_data['external_id']
            try:
                if not is_user_chat_admin(external_id, telegram_user_id):
                    form.add_error(
                        'external_id',
                        'You are not an admin in this Telegram chat.',
                    )
            except RuntimeError as exc:
                form.add_error('external_id', str(exc))
        if form.is_valid():
            channel = form.save()
            messages.success(request, 'Channel settings updated.')
            return redirect('channels:channel-detail', pk=channel.pk)
    else:
        form = ManagedChannelForm(instance=channel)

    return render(
        request,
        'dashboard/channel_detail.html',
        {
            'channel': channel,
            'form': form,
        },
    )
