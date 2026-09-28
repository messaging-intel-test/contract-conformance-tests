package com.oresoftware.common_desktop_infra

import android.content.Context
import androidx.work.Worker
import androidx.work.WorkerParameters

class HostingRepairWorker(appContext: Context, workerParams: WorkerParameters) : Worker(appContext, workerParams) {
    override fun doWork(): Result {
        if (MobileHostForegroundService.isRunning()) { return Result.success() }
        val preferences=applicationContext.getSharedPreferences(OresCommonMobileHostPlugin.PREFERENCES_NAME,Context.MODE_PRIVATE)
        val productId=preferences.getString("product_id",null); val originPort=preferences.getInt("origin_port",0); val hostingEnabled=preferences.getBoolean("hosting_enabled",false)
        if (!hostingEnabled || productId.isNullOrBlank() || originPort<=0) { return Result.success() }
        return try { MobileHostForegroundService.start(applicationContext,productId,originPort); Result.success() } catch (_: RuntimeException) { Result.retry() }
    }
}
