package com.oresoftware.common_desktop_infra

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.content.Context
import android.content.Intent
import android.os.Build
import android.os.IBinder
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat
import java.util.concurrent.atomic.AtomicBoolean

class MobileHostForegroundService : Service() {
    companion object {
        private const val CHANNEL_ID = "ores_mobile_host"
        private const val NOTIFICATION_ID = 43127
        private val running = AtomicBoolean(false)
        fun isRunning(): Boolean { return running.get() }
        fun start(context: Context, productId: String, originPort: Int) { val intent = Intent(context, MobileHostForegroundService::class.java).apply { putExtra("product_id", productId); putExtra("origin_port", originPort) }; ContextCompat.startForegroundService(context, intent) }
        fun stop(context: Context) { context.stopService(Intent(context, MobileHostForegroundService::class.java)) }
    }
    override fun onCreate() { super.onCreate(); createNotificationChannel(); running.set(true) }
    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int { val productId=intent?.getStringExtra("product_id")?:"ORES"; val originPort=intent?.getIntExtra("origin_port",0)?:0; startForeground(NOTIFICATION_ID, buildNotification(productId, originPort)); return START_STICKY }
    override fun onDestroy() { running.set(false); super.onDestroy() }
    override fun onBind(intent: Intent?): IBinder? { return null }
    private fun createNotificationChannel() { if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) { return }; val channel=NotificationChannel(CHANNEL_ID,"ORES mobile hosting",NotificationManager.IMPORTANCE_LOW).apply { description="Keeps an explicitly enabled local hosting node and edge relay active"; setShowBadge(false) }; getSystemService(NotificationManager::class.java).createNotificationChannel(channel) }
    private fun buildNotification(productId:String, originPort:Int):Notification { val icon=applicationInfo.icon.takeIf{it!=0}?:android.R.drawable.stat_notify_sync; return NotificationCompat.Builder(this,CHANNEL_ID).setSmallIcon(icon).setContentTitle("$productId hosting is active").setContentText("Local origin is available on 127.0.0.1:$originPort").setOngoing(true).setOnlyAlertOnce(true).setCategory(NotificationCompat.CATEGORY_SERVICE).build() }
}
