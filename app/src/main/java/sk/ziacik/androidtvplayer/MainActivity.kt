package sk.ziacik.androidtvplayer

import android.os.Bundle
import android.util.Log
import android.view.View
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import java.io.File
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.launch
import sk.ziacik.androidtvplayer.archive.StvrArchiveResolver
import sk.ziacik.androidtvplayer.channel.ArchiveProvider
import sk.ziacik.androidtvplayer.channel.SharedPreferencesChannelStore
import sk.ziacik.androidtvplayer.channel.ChannelCatalogRepository
import sk.ziacik.androidtvplayer.channel.ChannelCatalog
import sk.ziacik.androidtvplayer.channel.OkHttpChannelCatalogDownloader
import sk.ziacik.androidtvplayer.channel.TvChannel
import sk.ziacik.androidtvplayer.channel.EpgSourceId
import sk.ziacik.androidtvplayer.epg.CachedXmltvEpgRepository
import sk.ziacik.androidtvplayer.epg.OkHttpEpgDownloader
import sk.ziacik.androidtvplayer.epg.XmltvEpgSource
import sk.ziacik.androidtvplayer.player.Media3PlayerPort
import sk.ziacik.androidtvplayer.player.PlayerController
import sk.ziacik.androidtvplayer.resolver.ChannelResolver
import sk.ziacik.androidtvplayer.resolver.CnnPrimaNewsResolver
import sk.ziacik.androidtvplayer.resolver.CtResolver
import sk.ziacik.androidtvplayer.resolver.DirectResolver
import sk.ziacik.androidtvplayer.resolver.JojResolver
import sk.ziacik.androidtvplayer.resolver.OkHttpFreeviewClient
import sk.ziacik.androidtvplayer.resolver.OkHttpStvrClient
import sk.ziacik.androidtvplayer.resolver.NovaResolver
import sk.ziacik.androidtvplayer.resolver.StvrResolver
import sk.ziacik.androidtvplayer.resolver.StreamResolveException
import sk.ziacik.androidtvplayer.resolver.SweetTvResolver
import sk.ziacik.androidtvplayer.resolver.Ta3Resolver
import sk.ziacik.androidtvplayer.ui.AndroidTvPlayerTheme
import sk.ziacik.androidtvplayer.ui.OverlayController
import sk.ziacik.androidtvplayer.ui.PlayerScreen
import sk.ziacik.androidtvplayer.ui.UpdatePrompt
import sk.ziacik.androidtvplayer.update.AppUpdateState
import sk.ziacik.androidtvplayer.update.GithubAppUpdater
import sk.ziacik.androidtvplayer.update.UpdateInfo

class MainActivity : ComponentActivity() {
    private val appScope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val updateState = MutableStateFlow<AppUpdateState>(AppUpdateState.Hidden)
    private lateinit var playerController: PlayerController
    private lateinit var appUpdater: GithubAppUpdater
    private var pendingUpdateAfterPermission: UpdateInfo? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        @Suppress("DEPRECATION")
        window.decorView.systemUiVisibility =
            View.SYSTEM_UI_FLAG_FULLSCREEN or
                View.SYSTEM_UI_FLAG_HIDE_NAVIGATION or
                View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY

        appUpdater = GithubAppUpdater(this)

        val playerPort = Media3PlayerPort(this)
        val freeviewHttpClient = OkHttpFreeviewClient()
        val stvrHttpClient = OkHttpStvrClient()
        val stvrArchiveResolver = StvrArchiveResolver(stvrHttpClient)
        val resolver = ChannelResolver(
            resolveStvr = StvrResolver(stvrHttpClient)::resolve,
            resolveJoj = JojResolver(freeviewHttpClient)::resolve,
            resolveCt = CtResolver(freeviewHttpClient)::resolve,
            resolveTa3 = Ta3Resolver(freeviewHttpClient)::resolve,
            resolveNova = NovaResolver(freeviewHttpClient)::resolve,
            resolveCnnPrimaNews = CnnPrimaNewsResolver(freeviewHttpClient)::resolve,
            resolveSweetTv = SweetTvResolver(freeviewHttpClient)::resolve,
            resolveDirect = DirectResolver()::resolve,
        )
        val catalogRepository = ChannelCatalogRepository(
            seed = {
                runCatching {
                    assets.open("channels.json").bufferedReader().use { it.readText() }
                }.getOrNull()
            },
            cacheFile = File(filesDir, "channels.json"),
            download = OkHttpChannelCatalogDownloader(CHANNELS_URL)::download,
        )
        val catalog = ChannelCatalog(
            catalogRepository.loadOrNull()?.channels.orEmpty() + TvChannel.sweetTvChannels,
        )
        TvChannel.setRuntimeEntries(catalog.channels)
        val channelStore = SharedPreferencesChannelStore(this)
        val epgDirectory = File(filesDir, "epg")
        File(epgDirectory, "iptv-org-cz.xml.gz").delete()
        val epgRepository = CachedXmltvEpgRepository(
            sources = listOf(
                XmltvEpgSource(
                    id = EpgSourceId.OPEN_EPG,
                    cacheFile = File(epgDirectory, "open-epg.xml.gz"),
                    download = OkHttpEpgDownloader(OPEN_EPG_URL)::download,
                ),
                XmltvEpgSource(
                    id = EpgSourceId.SKYLINK,
                    cacheFile = File(epgDirectory, "skylink-a3b-a1.xml"),
                    download = OkHttpEpgDownloader(SKYLINK_EPG_URL)::download,
                ),
            ),
            diagnostics = { message, cause ->
                Log.e("AndroidTvPlayer", message, cause)
            },
        )
        val overlayController = OverlayController(appScope)
        playerController = PlayerController(
            scope = appScope,
            initialChannel = channelStore.load(catalog),
            resolve = resolver::resolve,
            playerPort = playerPort,
            resolveArchive = { channel, program ->
                when (channel.archive?.provider) {
                    ArchiveProvider.STVR -> stvrArchiveResolver.resolve(
                        channel = channel,
                        startsAtMs = requireNotNull(program.startsAtMs),
                    )
                    null -> throw StreamResolveException("Archive playback is not configured")
                }
            },
            epgRepository = epgRepository,
            onChannelSelected = channelStore::save,
            diagnostics = { message, cause ->
                Log.e("AndroidTvPlayer", message, cause)
            },
        )
        appScope.launch {
            runCatching {
                ChannelCatalog(
                    catalogRepository.refresh().channels + TvChannel.sweetTvChannels,
                )
            }
                .onSuccess { refreshedCatalog ->
                    val changed = TvChannel.entries != refreshedCatalog.channels
                    TvChannel.setRuntimeEntries(refreshedCatalog.channels)
                    if (changed) playerController.retry()
                }
                .onFailure { Log.w("AndroidTvPlayer", "Channel catalog refresh failed") }
        }

        setContent {
            AndroidTvPlayerTheme {
                val currentUpdateState by updateState.collectAsState()
                Box(Modifier.fillMaxSize()) {
                    PlayerScreen(
                        controller = playerController,
                        player = playerPort.player,
                        overlayController = overlayController,
                        epgRepository = epgRepository,
                        archiveAvailable = { channel, program ->
                            when (channel.archive?.provider) {
                                ArchiveProvider.STVR -> stvrArchiveResolver.isAvailable(channel, program)
                                null -> false
                            }
                        },
                        onExit = ::finish,
                    )
                    UpdatePrompt(
                        state = currentUpdateState,
                        onUpdate = {
                            val info = when (val state = updateState.value) {
                                is AppUpdateState.Available -> state.info
                                is AppUpdateState.Error -> state.info
                                else -> null
                            }
                            if (info != null) requestOrInstallUpdate(info)
                        },
                        onLater = { updateState.value = AppUpdateState.Hidden },
                        modifier = Modifier.align(Alignment.Center),
                    )
                }
            }
        }

        if (appUpdater.shouldUseSelfUpdater()) {
            appScope.launch {
                runCatching { appUpdater.checkForUpdate() }
                    .onSuccess { info ->
                        if (info != null) updateState.value = AppUpdateState.Available(info)
                    }
                    .onFailure { Log.w("AndroidTvPlayer", "Update check failed", it) }
            }
        }
    }

    override fun onStart() {
        super.onStart()
        playerController.start()
    }

    override fun onStop() {
        playerController.stop()
        super.onStop()
    }

    override fun onResume() {
        super.onResume()
        val pending = pendingUpdateAfterPermission ?: return
        if (appUpdater.canRequestPackageInstalls()) {
            pendingUpdateAfterPermission = null
            startUpdate(pending)
        }
    }

    override fun onDestroy() {
        playerController.release()
        appScope.cancel()
        super.onDestroy()
    }

    private fun requestOrInstallUpdate(info: UpdateInfo) {
        if (!appUpdater.canRequestPackageInstalls()) {
            pendingUpdateAfterPermission = info
            appUpdater.requestInstallPermission(this)
            return
        }
        startUpdate(info)
    }

    private fun startUpdate(info: UpdateInfo) {
        if (updateState.value is AppUpdateState.Downloading) return
        updateState.value = AppUpdateState.Downloading(info)
        appScope.launch {
            runCatching { appUpdater.download(info) }
                .onSuccess { apk ->
                    updateState.value = AppUpdateState.Hidden
                    appUpdater.install(this@MainActivity, apk)
                }
                .onFailure { error ->
                    Log.w("AndroidTvPlayer", "Update download failed", error)
                    updateState.value = AppUpdateState.Error(
                        info = info,
                        message = "Aktualizáciu sa nepodarilo stiahnuť alebo overiť.",
                    )
                }
        }
    }

    private companion object {
        const val SKYLINK_EPG_URL =
            "https://raw.githubusercontent.com/370network/skylink-xmltv/refs/heads/main/a3b_a1.xml"
        const val OPEN_EPG_URL = "https://www.open-epg.com/generate/jnapkTB7Wq.xml.gz"
        const val CHANNELS_URL = "https://plainraw.com/json/cbc833011422"
    }
}