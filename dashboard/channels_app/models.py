from django.db import models


class ManagedChannel(models.Model):
    class Platform(models.TextChoices):
        TELEGRAM = 'telegram', 'Telegram'
        DISCORD = 'discord', 'Discord'

    class ModerationMode(models.TextChoices):
        BALANCED = 'balanced', 'Balanced'
        STRICT = 'strict', 'Strict'
        PARANOID = 'paranoid', 'Paranoid'

    class BotStatus(models.TextChoices):
        ONLINE = 'online', 'Online'
        PAUSED = 'paused', 'Paused'
        OFFLINE = 'offline', 'Offline'

    name = models.CharField(max_length=120)
    external_id = models.CharField(max_length=64, unique=True)
    platform = models.CharField(
        max_length=16,
        choices=Platform.choices,
        default=Platform.TELEGRAM,
    )
    owner = models.CharField(max_length=120, blank=True)
    moderation_mode = models.CharField(
        max_length=16,
        choices=ModerationMode.choices,
        default=ModerationMode.BALANCED,
    )
    bot_status = models.CharField(
        max_length=16,
        choices=BotStatus.choices,
        default=BotStatus.ONLINE,
    )
    whitelist_guard_enabled = models.BooleanField(default=True)
    vector_fallback_enabled = models.BooleanField(default=True)
    messages_scanned = models.PositiveBigIntegerField(default=0)
    blocked_messages = models.PositiveBigIntegerField(default=0)
    last_event_at = models.DateTimeField(null=True, blank=True)
    created_at = models.DateTimeField(auto_now_add=True)
    updated_at = models.DateTimeField(auto_now=True)

    class Meta:
        ordering = ('name',)

    def __str__(self) -> str:
        return f'{self.name} ({self.get_platform_display()})'

    @property
    def block_rate(self) -> float:
        if self.messages_scanned == 0:
            return 0.0
        return (self.blocked_messages / self.messages_scanned) * 100
