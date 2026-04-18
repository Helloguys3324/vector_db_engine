from django import forms

from .models import ManagedChannel


class ManagedChannelForm(forms.ModelForm):
    class Meta:
        model = ManagedChannel
        fields = (
            'name',
            'external_id',
            'owner',
            'moderation_mode',
            'bot_status',
            'whitelist_guard_enabled',
            'vector_fallback_enabled',
        )

    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        for name, field in self.fields.items():
            widget = field.widget
            if isinstance(widget, forms.CheckboxInput):
                widget.attrs['class'] = 'form-check-input'
                continue
            css_class = 'form-select' if isinstance(widget, forms.Select) else 'form-control'
            widget.attrs['class'] = css_class
            if name == 'external_id':
                widget.attrs['placeholder'] = 'e.g. -1001234567890'

    def save(self, commit=True):
        instance = super().save(commit=False)
        instance.platform = ManagedChannel.Platform.TELEGRAM
        if commit:
            instance.save()
        return instance
