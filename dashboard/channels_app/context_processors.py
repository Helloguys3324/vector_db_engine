from django.conf import settings


def telegram_dashboard_context(request):
    telegram_user = request.session.get('telegram_user')
    return {
        'telegram_user': telegram_user,
        'telegram_login_bot_username': settings.DASHBOARD_TELEGRAM_BOT_USERNAME,
    }
