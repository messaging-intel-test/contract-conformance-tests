package com.oresoftware.common_desktop_infra

import android.content.Context
import androidx.core.app.NotificationManagerCompat
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import io.flutter.embedding.engine.plugins.FlutterPlugin
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel
import java.util.concurrent.TimeUnit
import kotlin.math.ceil
import kotlin.math.max

class OresCommonMobileHostPlugin : FlutterPlugin, MethodChannel.MethodCallHandler {
    companion object {
        const val PREFERENCES_NAME = "ores_common_mobile_host"
        private const val REPAIR_WORK_NAME = "ores-mobile-host-repair"
        private const val NUDGE_WORK_NAME = "ores-mobile-host-keepalive-nudge"
        private const val MIN_PERIODIC_WAKE_MINUTES = 15L
        private const val MIN_NUDGES_PER_DAY = 3
        private const val MAX_NUDGES_PER_DAY = 5
        private const val MAX_TCP_PORT = 65535
    }

    private lateinit var context: Context
    private lateinit var channel: MethodChannel

    override fun onAttachedToEngine(binding: FlutterPlugin.FlutterPluginBinding) {
        context = binding.applicationContext
        channel = MethodChannel(
            binding.binaryMessenger,
            "ores_common_mobile_host/background",
        )
        channel.setMethodCallHandler(this)
    }

    override fun onDetachedFromEngine(binding: FlutterPlugin.FlutterPluginBinding) {
        channel.setMethodCallHandler(null)
    }

    override fun onMethodCall(call: MethodCall, result: MethodChannel.Result) {
        when (call.method) {
            "capabilities" -> {
                val notificationsEnabled = NotificationManagerCompat
                    .from(context)
                    .areNotificationsEnabled()

                result.success(
                    mapOf(
                        "platform" to "android",
                        "supports_persistent_origin" to true,
                        "supports_wake_and_drain" to true,
                        "supports_background_push" to false,
                        "minimum_repair_wake_minutes" to MIN_PERIODIC_WAKE_MINUTES,
                        "background_task_registered" to true,
                        "notifications_enabled" to notificationsEnabled,
                    ),
                )
            }

            "startPersistentHosting" -> {
                startPersistentHosting(call, result)
            }

            "stopPersistentHosting" -> {
                stopPersistentHosting(result)
            }

            "scheduleRepairWake" -> {
                scheduleRepairWake(call, result)
            }

            "configureKeepalivePrompts" -> {
                configureKeepalivePrompts(call, result)
            }

            else -> {
                result.notImplemented()
            }
        }
    }

    private fun startPersistentHosting(call: MethodCall, result: MethodChannel.Result) {
        val productId = call.argument<String>("product_id")
        val originPort = call.argument<Int>("origin_port")

        if (
            productId.isNullOrBlank() ||
            originPort == null ||
            originPort <= 0 ||
            originPort > MAX_TCP_PORT
        ) {
            result.error(
                "invalid_host_config",
                "product_id and a valid origin_port are required",
                null,
            )
            return
        }

        context.getSharedPreferences(PREFERENCES_NAME, Context.MODE_PRIVATE)
            .edit()
            .putString("product_id", productId)
            .putInt("origin_port", originPort)
            .putBoolean("hosting_enabled", true)
            .apply()

        try {
            MobileHostForegroundService.start(context, productId, originPort)
            result.success(null)
        } catch (error: RuntimeException) {
            result.error(
                "foreground_service_start_failed",
                error.message,
                null,
            )
        }
    }

    private fun stopPersistentHosting(result: MethodChannel.Result) {
        context.getSharedPreferences(PREFERENCES_NAME, Context.MODE_PRIVATE)
            .edit()
            .putBoolean("hosting_enabled", false)
            .apply()

        WorkManager.getInstance(context).cancelUniqueWork(REPAIR_WORK_NAME)
        MobileHostForegroundService.stop(context)
        result.success(null)
    }

    private fun scheduleRepairWake(call: MethodCall, result: MethodChannel.Result) {
        val minimumDelaySeconds = call.argument<Number>("minimum_delay_seconds")
            ?.toLong()
            ?: 0L

        if (minimumDelaySeconds < 0L) {
            result.error(
                "invalid_repair_delay",
                "minimum_delay_seconds must not be negative",
                null,
            )
            return
        }

        val requestedMinutes = ceil(minimumDelaySeconds / 60.0).toLong()
        val intervalMinutes = max(MIN_PERIODIC_WAKE_MINUTES, requestedMinutes)

        val request = PeriodicWorkRequestBuilder<HostingRepairWorker>(
            intervalMinutes,
            TimeUnit.MINUTES,
        ).build()

        WorkManager.getInstance(context).enqueueUniquePeriodicWork(
            REPAIR_WORK_NAME,
            ExistingPeriodicWorkPolicy.UPDATE,
            request,
        )

        result.success(
            mapOf(
                "scheduled_interval_minutes" to intervalMinutes,
                "exact" to false,
            ),
        )
    }

    private fun configureKeepalivePrompts(call: MethodCall, result: MethodChannel.Result) {
        val enabled = call.argument<Boolean>("enabled") ?: false
        val promptsPerDay = call.argument<Int>("prompts_per_day") ?: 4

        if (promptsPerDay < MIN_NUDGES_PER_DAY || promptsPerDay > MAX_NUDGES_PER_DAY) {
            result.error(
                "invalid_prompt_frequency",
                "prompts_per_day must be between 3 and 5",
                null,
            )
            return
        }

        val workManager = WorkManager.getInstance(context)

        if (!enabled) {
            context.getSharedPreferences(PREFERENCES_NAME, Context.MODE_PRIVATE)
                .edit()
                .putBoolean("keepalive_prompts_enabled", false)
                .putInt("keepalive_prompts_per_day", promptsPerDay)
                .apply()
            workManager.cancelUniqueWork(NUDGE_WORK_NAME)
            result.success(null)
            return
        }

        if (!NotificationManagerCompat.from(context).areNotificationsEnabled()) {
            result.error(
                "notifications_unavailable",
                "Keepalive prompts require notification permission in the host app",
                null,
            )
            return
        }

        context.getSharedPreferences(PREFERENCES_NAME, Context.MODE_PRIVATE)
            .edit()
            .putBoolean("keepalive_prompts_enabled", true)
            .putInt("keepalive_prompts_per_day", promptsPerDay)
            .apply()

        val intervalMinutes = 24L * 60L / promptsPerDay.toLong()
        val request = PeriodicWorkRequestBuilder<HostingNudgeWorker>(
            intervalMinutes,
            TimeUnit.MINUTES,
        ).build()

        workManager.enqueueUniquePeriodicWork(
            NUDGE_WORK_NAME,
            ExistingPeriodicWorkPolicy.UPDATE,
            request,
        )

        result.success(
            mapOf(
                "prompts_per_day" to promptsPerDay,
                "interval_minutes" to intervalMinutes,
                "exact" to false,
            ),
        )
    }
}
