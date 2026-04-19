from django.urls import path

from . import views

app_name = 'channels'

urlpatterns = [
    path('', views.login_page, name='login'),
    path('login/', views.login_page, name='login-legacy'),
    path('logout/', views.logout_page, name='logout'),
    path('auth/telegram/callback/', views.telegram_auth_callback, name='telegram-auth-callback'),
    path('dashboard/', views.dashboard_home, name='home'),
    path('channels/new/', views.channel_create, name='channel-create'),
    path('channels/<int:pk>/', views.channel_detail, name='channel-detail'),
    path('analytics/', views.analytics_page, name='analytics'),
    path('offenders/', views.offenders_page, name='offenders'),
    path('connect/', views.connect_page, name='connect'),
]
