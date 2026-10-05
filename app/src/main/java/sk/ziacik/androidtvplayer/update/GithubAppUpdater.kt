package sk.ziacik.androidtvplayer.update

import android.app.Activity
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.provider.Settings
import androidx.core.content.FileProvider
import java.io.File
import java.security.MessageDigest
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import okhttp3.OkHttpClient
import okhttp3.Request
import org.json.JSONObject

data class UpdateInfo(
    val versionCode: Int,
    val versionName: String,
    val apkName: String,
    val apkUrl: String,
    val sha256: String,
)

sealed interface AppUpdateState {
    data object Hidden : AppUpdateState
    data class Available(val info: UpdateInfo) : AppUpdateState
    data class Downloading(val info: UpdateInfo) : AppUpdateState
    data class Error(val info: UpdateInfo, val message: String) : AppUpdateState
}

class GithubAppUpdater(
    private val context: Context,
    private val client: OkHttpClient = OkHttpClient(),
) {
    fun shouldUseSelfUpdater(): Boolean = !isFdroidInstallerPackage(installerPackageName())

    suspend fun checkForUpdate(): UpdateInfo? = withContext(Dispatchers.IO) {
        val release = getJson(LATEST_RELEASE_URL)
        val assets = release.getJSONArray("assets")

        val metadataUrl = (0 until assets.length())
            .asSequence()
            .map { assets.getJSONObject(it) }
            .firstOrNull { it.getString("name") == UPDATE_METADATA_NAME }
            ?.getString("browser_download_url")
            ?: return@withContext null

        val metadata = getJson(metadataUrl)
        val versionCode = metadata.getInt("versionCode")
        if (versionCode <= currentVersionCode()) return@withContext null

        val apkName = metadata.getString("apkName")
        val apkUrl = (0 until assets.length())
            .asSequence()
            .map { assets.getJSONObject(it) }
            .firstOrNull { it.getString("name") == apkName }
            ?.getString("browser_download_url")
            ?: error("Release asset $apkName is missing")

        UpdateInfo(
            versionCode = versionCode,
            versionName = metadata.getString("versionName"),
            apkName = apkName,
            apkUrl = apkUrl,
            sha256 = metadata.getString("sha256").lowercase(),
        )
    }

    suspend fun download(info: UpdateInfo): File = withContext(Dispatchers.IO) {
        val request = Request.Builder().url(info.apkUrl).build()
        val targetDir = File(context.cacheDir, "updates").apply { mkdirs() }
        val target = File(targetDir, info.apkName)
        val digest = MessageDigest.getInstance("SHA-256")

        client.newCall(request).execute().use { response ->
            if (!response.isSuccessful) error("APK download failed: HTTP ${response.code}")
            val body = response.body ?: error("APK download returned an empty body")
            body.byteStream().use { input ->
                target.outputStream().use { output ->
                    val buffer = ByteArray(DEFAULT_BUFFER_SIZE)
                    while (true) {
                        val read = input.read(buffer)
                        if (read < 0) break
                        digest.update(buffer, 0, read)
                        output.write(buffer, 0, read)
                    }
                }
            }
        }

        val actualSha256 = digest.digest().joinToString("") { "%02x".format(it) }
        if (!actualSha256.equals(info.sha256, ignoreCase = true)) {
            target.delete()
            error("Downloaded APK checksum does not match the release metadata")
        }
        target
    }

    fun canRequestPackageInstalls(): Boolean =
        Build.VERSION.SDK_INT < Build.VERSION_CODES.O ||
            context.packageManager.canRequestPackageInstalls()

    fun requestInstallPermission(activity: Activity) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        activity.startActivity(
            Intent(
                Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES,
                Uri.parse("package:${context.packageName}"),
            ),
        )
    }

    fun install(activity: Activity, apk: File) {
        val uri = FileProvider.getUriForFile(
            context,
            "${context.packageName}.fileprovider",
            apk,
        )
        activity.startActivity(
            Intent(Intent.ACTION_VIEW)
                .setDataAndType(uri, APK_MIME_TYPE)
                .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION),
        )
    }

    @Suppress("DEPRECATION")
    private fun currentVersionCode(): Long {
        val info = context.packageManager.getPackageInfo(context.packageName, 0)
        return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            info.longVersionCode
        } else {
            info.versionCode.toLong()
        }
    }

    @Suppress("DEPRECATION")
    private fun installerPackageName(): String? =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            context.packageManager
                .getInstallSourceInfo(context.packageName)
                .installingPackageName
        } else {
            context.packageManager.getInstallerPackageName(context.packageName)
        }

    private fun getJson(url: String): JSONObject {
        val request = Request.Builder()
            .url(url)
            .header("Accept", "application/vnd.github+json")
            .build()
        return client.newCall(request).execute().use { response ->
            if (!response.isSuccessful) error("Update check failed: HTTP ${response.code}")
            val body = response.body?.string() ?: error("Update check returned an empty body")
            JSONObject(body)
        }
    }

    private companion object {
        const val LATEST_RELEASE_URL =
            "https://api.github.com/repos/ziacik/android-tv-player/releases/latest"
        const val UPDATE_METADATA_NAME = "kanalik-update.json"
        const val APK_MIME_TYPE = "application/vnd.android.package-archive"
    }
}

internal fun isFdroidInstallerPackage(packageName: String?): Boolean =
    packageName == "org.fdroid.fdroid" || packageName == "org.fdroid.basic"
