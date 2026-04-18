from django.contrib import admin

from .models import ManagedChannel


@admin.register(ManagedChannel)
class ManagedChannelAdmin(admin.ModelAdmin):
    list_display = (
        'name',
        'platform',
        'external_id',
        'moderation_mode',
        'bot_status',
        'messages_scanned',
        'blocked_messages',
        'updated_at',
    )
    list_filter = (
        'platform',
        'moderation_mode',
        'bot_status',
        'whitelist_guard_enabled',
        'vector_fallback_enabled',
    )
    search_fields = ('name', 'external_id', 'owner')
