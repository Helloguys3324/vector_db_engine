from functools import wraps

from django.conf import settings
from django.contrib import messages
from django.core.exceptions import PermissionDenied
from django.db.models import Sum
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

    # Try to get offenders for this channel
    try:
        from .models import OffenderStats, ViolationEvent
        offenders = OffenderStats.objects.filter(channel=channel).order_by('-violation_count')[:10]
        recent_events = ViolationEvent.objects.filter(channel=channel).order_by('-occurred_at')[:8]
        # Top words aggregation (basic)
        from django.db.models import Count
        top_words_qs = (
            ViolationEvent.objects.filter(channel=channel)
            .exclude(matched_word='')
            .values('matched_word')
            .annotate(count=Count('id'))
            .order_by('-count')[:10]
        )
        max_count = top_words_qs[0]['count'] if top_words_qs else 1
        top_words = [
            {'word': w['matched_word'], 'count': w['count'], 'percent': int(w['count'] / max_count * 100)}
            for w in top_words_qs
        ]
    except Exception:
        offenders = []
        recent_events = []
        top_words = []

    return render(
        request,
        'dashboard/channel_detail.html',
        {
            'channel': channel,
            'form': form,
            'offenders': offenders,
            'recent_events': recent_events,
            'top_words': top_words,
        },
    )


@telegram_login_required
def analytics_page(request):
    telegram_user_id = _current_telegram_user_id(request)
    all_channels = ManagedChannel.objects.filter(platform=ManagedChannel.Platform.TELEGRAM)
    channels = []
    for ch in all_channels:
        try:
            if is_user_chat_admin(ch.external_id, telegram_user_id):
                channels.append(ch)
        except RuntimeError:
            pass

    total_scanned = sum(c.messages_scanned for c in channels)
    total_blocked = sum(c.blocked_messages for c in channels)
    total_channels = len(channels)
    telegram_count = len([c for c in channels if c.platform == 'telegram'])
    discord_count  = len([c for c in channels if c.platform == 'discord'])

    try:
        from .models import OffenderStats
        global_offenders = OffenderStats.objects.filter(
            channel__in=channels
        ).order_by('-violation_count')[:10]
        total_offenders = OffenderStats.objects.filter(channel__in=channels).values('telegram_user_id').distinct().count()
    except Exception:
        global_offenders = []
        total_offenders = 0

    return render(request, 'dashboard/analytics.html', {
        'total_scanned': total_scanned,
        'total_blocked': total_blocked,
        'total_channels': total_channels,
        'total_offenders': total_offenders,
        'telegram_count': max(telegram_count, 1),
        'discord_count': discord_count,
        'global_offenders': global_offenders,
    })


@telegram_login_required
def offenders_page(request):
    telegram_user_id = _current_telegram_user_id(request)
    all_channels = ManagedChannel.objects.filter(platform=ManagedChannel.Platform.TELEGRAM)
    channels = []
    for ch in all_channels:
        try:
            if is_user_chat_admin(ch.external_id, telegram_user_id):
                channels.append(ch)
        except RuntimeError:
            pass

    try:
        from .models import OffenderStats
        offenders = OffenderStats.objects.filter(channel__in=channels).order_by('-violation_count')[:50]
    except Exception:
        offenders = []

    return render(request, 'dashboard/offenders.html', {'offenders': offenders})


@telegram_login_required
def connect_page(request):
    platform = request.GET.get('platform', '')
    step = int(request.GET.get('step', 1))
    bot_username = ''
    try:
        bot_username = resolve_telegram_login_bot_username()
    except Exception:
        pass
    return render(request, 'dashboard/connect.html', {
        'platform': platform,
        'step': step,
        'bot_username': bot_username,
        'discord_oauth_url': '',  # TODO: configure Discord OAuth URL
    })
