package com.oresoftware.common_desktop_infra

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.work.CoroutineWorker
import androidx.work.WorkerParameters

class HostingNudgeWorker(appContext: Context, workerParams: WorkerParameters) : CoroutineWorker(appContext, workerParams) {
    companion object { private const val CHANNEL_ID="ores_mobile_host_keepalive"; private const val NOTIFICATION_ID=0x4f524553 }
    override suspend fun doWork(): Result {
        val preferences=applicationContext.getSharedPreferences(OresCommonMobileHostPlugin.PREFERENCES_NAME,Context.MODE_PRIVATE)
        if (!preferences.getBoolean("keepalive_prompts_enabled",false)) { return Result.success() }
        val notificationManager=NotificationManagerCompat.from(applicationContext); if (!notificationManager.areNotificationsEnabled()) { return Result.success() }; createNotificationChannel()
        val launchIntent=applicationContext.packageManager.getLaunchIntentForPackage(applicationContext.packageName)?:return Result.success()
        val pendingIntent=PendingIntent.getActivity(applicationContext,0,launchIntent,PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
        val productId=preferences.getString("product_id",null)?:"ORES"
        val notification=NotificationCompat.Builder(applicationContext,CHANNEL_ID).setSmallIcon(android.R.drawable.stat_notify_sync).setContentTitle("Keep $productId local hosting available").setContentText("Tap to reopen the app and refresh its local server/tunnel session.").setContentIntent(pendingIntent).setAutoCancel(true).setPriority(NotificationCompat.PRIORITY_DEFAULT).build()
        notificationManager.notify(NOTIFICATION_ID,notification); return Result.success()
    }
    private fun createNotificationChannel() { if (Build.VERSION.SDK_INT<Build.VERSION_CODES.O){return}; val manager=applicationContext.getSystemService(NotificationManager::class.java); val channel=NotificationChannel(CHANNEL_ID,"Local hosting keepalive",NotificationManager.IMPORTANCE_DEFAULT); channel.description="Optional reminders to reopen ORES hosting apps and refresh local server/tunnel sessions."; manager.createNotificationChannel(channel) }
}
