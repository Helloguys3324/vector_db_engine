import hashlib
import hmac
from unittest.mock import patch

from django.test import TestCase, override_settings
from django.urls import reverse

from .models import ManagedChannel


def _sign_telegram_payload(payload: dict[str, str], bot_token: str) -> str:
    data_check_string = '\n'.join(f'{k}={payload[k]}' for k in sorted(payload))
    secret = hashlib.sha256(bot_token.encode('utf-8')).digest()
    return hmac.new(secret, data_check_string.encode('utf-8'), hashlib.sha256).hexdigest()


@override_settings(
    DASHBOARD_TELEGRAM_BOT_TOKEN='123456:telegram-test-token',
    DASHBOARD_TELEGRAM_BOT_USERNAME='test_dashboard_bot',
)
class DashboardViewsTests(TestCase):
    def _set_telegram_session(self, user_id=99):
        session = self.client.session
        session['telegram_user'] = {
            'id': user_id,
            'first_name': 'Test',
            'last_name': 'User',
            'username': 'testuser',
            'photo_url': '',
        }
        session.save()

    def test_landing_page_renders(self):
        response = self.client.get(reverse('channels:login'))
        self.assertEqual(response.status_code, 200)
        self.assertContains(response, 'Log in through Telegram')
        self.assertNotContains(response, '>Admin<')

    @override_settings(DASHBOARD_TELEGRAM_BOT_USERNAME='')
    def test_landing_uses_getme_username_when_not_configured(self):
        with patch(
            'channels_app.views.resolve_telegram_login_bot_username',
            return_value='autodetected_bot_name',
        ):
            response = self.client.get(reverse('channels:login'))
        self.assertEqual(response.status_code, 200)
        self.assertContains(response, 'autodetected_bot_name')

    def test_home_requires_telegram_login(self):
        response = self.client.get(reverse('channels:home'))
        self.assertEqual(response.status_code, 302)
        self.assertTrue(response.url.startswith(reverse('channels:login')))

    def test_telegram_callback_creates_session(self):
        payload = {
            'id': '12345',
            'first_name': 'Alice',
            'username': 'alice_admin',
            'auth_date': '1735689600',
        }
        payload['hash'] = _sign_telegram_payload(
            payload,
            '123456:telegram-test-token',
        )
        with patch('channels_app.telegram.time.time', return_value=1735689600.0):
            response = self.client.get(
                reverse('channels:telegram-auth-callback'),
                data=payload,
            )
        self.assertEqual(response.status_code, 302)
        self.assertEqual(response.url, reverse('channels:home'))
        self.assertEqual(self.client.session['telegram_user']['id'], 12345)

    def test_home_shows_only_admin_chats(self):
        self._set_telegram_session(user_id=42)
        allowed = ManagedChannel.objects.create(name='Allowed Chat', external_id='-100100')
        denied = ManagedChannel.objects.create(name='Denied Chat', external_id='-100200')

        def _fake_admin(chat_id, _user_id):
            return chat_id == allowed.external_id

        with patch('channels_app.views.is_user_chat_admin', side_effect=_fake_admin):
            response = self.client.get(reverse('channels:home'))

        self.assertEqual(response.status_code, 200)
        self.assertContains(response, allowed.name)
        self.assertNotContains(response, denied.name)

    def test_channel_detail_requires_admin_rights(self):
        self._set_telegram_session(user_id=10)
        channel = ManagedChannel.objects.create(
            name='Test Channel',
            external_id='-1009000000001',
        )
        with patch('channels_app.views.is_user_chat_admin', return_value=False):
            response = self.client.get(reverse('channels:channel-detail', args=[channel.pk]))
        self.assertEqual(response.status_code, 403)
