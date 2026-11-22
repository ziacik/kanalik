package sk.ziacik.androidtvplayer.ui

import android.text.format.DateFormat
import android.view.KeyEvent
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.input.TextFieldLineLimits
import androidx.compose.foundation.text.input.rememberTextFieldState
import androidx.compose.material3.Button
import androidx.compose.material3.OutlinedSecureTextField
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.media3.common.Player
import androidx.media3.common.util.UnstableApi
import androidx.media3.ui.AspectRatioFrameLayout
import androidx.media3.ui.PlayerView
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import sk.ziacik.androidtvplayer.channel.TvChannel
import sk.ziacik.androidtvplayer.epg.EpgRepository
import sk.ziacik.androidtvplayer.player.PlayerController
import sk.ziacik.androidtvplayer.player.PlayerUiState
import sk.ziacik.androidtvplayer.resolver.ProgramMetadata

@Composable
@androidx.annotation.OptIn(UnstableApi::class)
fun PlayerScreen(
    controller: PlayerController,
    player: Player,
    overlayController: OverlayController,
    onExit: () -> Unit,
    modifier: Modifier = Modifier,
    epgRepository: EpgRepository = EpgRepository { _, _ -> null },
    archiveAvailable: suspend (TvChannel, ProgramMetadata) -> Boolean = { _, _ -> false },
    onSaveMarkizaCredentials: (String, String) -> Unit = { _, _ -> },
) {
    val state by controller.state.collectAsState()
    val streamHost by controller.streamHost.collectAsState()
    val overlayVisible by overlayController.visible.collectAsState()
    var focusedControl by remember { mutableStateOf(FocusedControl.TIMELINE) }
    var seekPreviewMs by remember { mutableStateOf<Long?>(null) }
    var scrubPositionMs by remember { mutableStateOf<Long?>(null) }
    var miniEpgVisible by remember { mutableStateOf(false) }
    var miniEpgSelectedChannel by remember { mutableStateOf<TvChannel?>(null) }
    var miniEpgProgrammes by remember { mutableStateOf<Map<String, ProgramMetadata>>(emptyMap()) }
    var miniEpgAttemptedChannels by remember { mutableStateOf<Set<String>>(emptySet()) }
    var miniEpgArchiveStates by remember {
        mutableStateOf<Map<MiniEpgProgrammeKey, MiniEpgArchiveState>>(emptyMap())
    }
    var miniEpgNowMs by remember { mutableStateOf(System.currentTimeMillis()) }
    val focusRequester = remember { FocusRequester() }
    val commandMapper = remember { RemoteCommandMapper() }
    val numericInputScope = rememberCoroutineScope()
    val numericInput = remember(controller, overlayController, numericInputScope) {
        NumericChannelInput(
            scope = numericInputScope,
            onChannelSelected = { channel ->
                overlayController.showUntilProgramTitleReady()
                controller.selectChannel(channel)
            },
        )
    }
    val numericDigits by numericInput.digits.collectAsState()
    val context = LocalContext.current
    val timeFormat = remember(context) { DateFormat.getTimeFormat(context) }
    val seekTimeFormat = remember(context) {
        SimpleDateFormat(
            if (DateFormat.is24HourFormat(context)) "HH:mm:ss" else "h:mm:ss a",
            Locale.getDefault(),
        )
    }

    fun loadMiniEpgProgramme(channel: TvChannel, atMs: Long) {
        numericInputScope.launch {
            val programme = try {
                epgRepository.currentProgram(channel, atMs)
            } catch (error: CancellationException) {
                throw error
            } catch (_: Exception) {
                null
            }
            if (
                miniEpgVisible &&
                (miniEpgSelectedChannel ?: state.channel).storageKey == channel.storageKey &&
                programme != null &&
                programme.title.isNotBlank()
            ) {
                miniEpgProgrammes = miniEpgProgrammes + (channel.storageKey to programme)
            }
        }
    }

    LaunchedEffect(Unit) {
        focusRequester.requestFocus()
    }

    DisposableEffect(numericInput) {
        onDispose { numericInput.cancel() }
    }

    LaunchedEffect(state) {
        if (state.hasResolvedProgramTitle()) {
            overlayController.onProgramTitleReady()
        }
    }

    LaunchedEffect(overlayVisible, state is PlayerUiState.Ready) {
        if (!overlayVisible || state !is PlayerUiState.Ready) return@LaunchedEffect
        while (true) {
            controller.refreshPlaybackSnapshot()
            delay(1_000L)
        }
    }

    LaunchedEffect(seekPreviewMs, scrubPositionMs) {
        if (seekPreviewMs == null || scrubPositionMs != null) return@LaunchedEffect
        delay(SEEK_PREVIEW_DURATION_MS)
        seekPreviewMs = null
    }

    LaunchedEffect(miniEpgVisible) {
        if (!miniEpgVisible) return@LaunchedEffect
        while (true) {
            miniEpgNowMs = System.currentTimeMillis()
            delay(1_000L)
        }
    }

    LaunchedEffect(
        miniEpgVisible,
        miniEpgSelectedChannel?.storageKey,
        state.channel.storageKey,
    ) {
        if (!miniEpgVisible) return@LaunchedEffect
        val selected = miniEpgSelectedChannel ?: state.channel
        val visibleChannels = buildMiniEpgRows(
            channels = TvChannel.entries,
            currentChannel = state.channel,
            selectedChannel = selected,
            programmes = miniEpgProgrammes,
            nowMs = miniEpgNowMs,
            archiveStates = miniEpgArchiveStates,
        ).map { it.channel }
        val missingChannels = visibleChannels.filter { channel ->
            channel.storageKey !in miniEpgAttemptedChannels
        }

        for (channel in missingChannels) {
            val programme = try {
                epgRepository.currentProgram(channel, System.currentTimeMillis())
            } catch (error: CancellationException) {
                throw error
            } catch (_: Exception) {
                null
            }
            miniEpgAttemptedChannels = miniEpgAttemptedChannels + channel.storageKey
            if (programme != null && programme.title.isNotBlank()) {
                miniEpgProgrammes = miniEpgProgrammes + (channel.storageKey to programme)
            }
        }
    }

    LaunchedEffect(
        miniEpgVisible,
        miniEpgSelectedChannel?.storageKey,
        state.channel.storageKey,
        miniEpgProgrammes,
    ) {
        if (!miniEpgVisible) return@LaunchedEffect
        val rows = buildMiniEpgRows(
            channels = TvChannel.entries,
            currentChannel = state.channel,
            selectedChannel = miniEpgSelectedChannel ?: state.channel,
            programmes = miniEpgProgrammes,
            nowMs = miniEpgNowMs,
            archiveStates = miniEpgArchiveStates,
        )
        for (row in rows.filter { it.archiveState == MiniEpgArchiveState.LOADING }) {
            val programme = miniEpgProgrammes[row.channel.storageKey] ?: continue
            val key = miniEpgProgrammeKey(row.channel, programme)
            if (key in miniEpgArchiveStates) continue
            val available = try {
                archiveAvailable(row.channel, programme)
            } catch (error: CancellationException) {
                throw error
            } catch (_: Exception) {
                false
            }
            if (!miniEpgVisible) return@LaunchedEffect
            miniEpgArchiveStates = miniEpgArchiveStates + (
                key to if (available) {
                    MiniEpgArchiveState.AVAILABLE
                } else {
                    MiniEpgArchiveState.UNAVAILABLE
                }
            )
        }
    }

    BackHandler {
        when {
            miniEpgVisible -> {
                miniEpgVisible = false
                miniEpgSelectedChannel = null
            }
            overlayVisible -> {
                seekPreviewMs = null
                scrubPositionMs = null
                overlayController.hide()
            }
            else -> onExit()
        }
    }

    Box(
        modifier = modifier
            .fillMaxSize()
            .background(Color.Black)
            .focusRequester(focusRequester)
            .onPreviewKeyEvent { event ->
                val keyCode = event.nativeKeyEvent.keyCode
                if (!shouldHandleRemoteKeyEvent(
                        action = event.nativeKeyEvent.action,
                        keyCode = keyCode,
                        miniEpgVisible = miniEpgVisible,
                        overlayVisible = overlayVisible,
                        focusedControl = focusedControl,
                    )
                ) {
                    return@onPreviewKeyEvent false
                }

                val command = commandMapper.map(
                    keyCode = keyCode,
                    overlayVisible = overlayVisible,
                    focusedControl = focusedControl,
                    miniEpgVisible = miniEpgVisible,
                )
                if (state is PlayerUiState.CredentialsRequired && !command.isChannelSelection()) {
                    return@onPreviewKeyEvent false
                }
                val retryable = state is PlayerUiState.Unavailable || state is PlayerUiState.Error
                if (!miniEpgVisible && retryable && keyCode.isCenterKey()) {
                    controller.retry()
                    return@onPreviewKeyEvent true
                }

                val horizontalSeekDirection = when (keyCode) {
                    KeyEvent.KEYCODE_DPAD_LEFT -> -1L
                    KeyEvent.KEYCODE_DPAD_RIGHT -> 1L
                    else -> null
                }
                if (
                    horizontalSeekDirection != null &&
                    !miniEpgVisible &&
                    (!overlayVisible || focusedControl == FocusedControl.TIMELINE)
                ) {
                    when (event.nativeKeyEvent.action) {
                        KeyEvent.ACTION_DOWN -> {
                            val repeatCount = event.nativeKeyEvent.repeatCount
                            if (overlayVisible || repeatCount > 0 || scrubPositionMs != null) {
                                controller.previewSeek(
                                    fromPositionMs = scrubPositionMs,
                                    deltaMs = horizontalSeekDirection * seekScrubStepMs(repeatCount),
                                )?.let { preview ->
                                    scrubPositionMs = preview.positionMs
                                    seekPreviewMs = preview.clockTimeMs
                                    focusedControl = FocusedControl.TIMELINE
                                    overlayController.show(OverlayController.NORMAL_TIMEOUT_MS)
                                }
                            }
                            return@onPreviewKeyEvent true
                        }
                        KeyEvent.ACTION_UP -> {
                            val pendingPositionMs = scrubPositionMs
                            seekPreviewMs = if (pendingPositionMs != null) {
                                controller.commitSeek(pendingPositionMs) ?: seekPreviewMs
                            } else if (horizontalSeekDirection < 0L) {
                                controller.seekBack()
                            } else {
                                controller.seekForward()
                            }
                            scrubPositionMs = null
                            controller.refreshPlaybackSnapshot()
                            overlayController.show(OverlayController.NORMAL_TIMEOUT_MS)
                            return@onPreviewKeyEvent true
                        }
                    }
                }

                when (command) {
                    is RemoteCommand.NumericDigit -> {
                        miniEpgVisible = false
                        miniEpgSelectedChannel = null
                        numericInput.append(command.digit)
                    }
                    RemoteCommand.ChannelUp -> {
                        miniEpgVisible = false
                        miniEpgSelectedChannel = null
                        seekPreviewMs = null
                        scrubPositionMs = null
                        overlayController.showUntilProgramTitleReady()
                        controller.channelUp()
                    }
                    RemoteCommand.ChannelDown -> {
                        miniEpgVisible = false
                        miniEpgSelectedChannel = null
                        seekPreviewMs = null
                        scrubPositionMs = null
                        overlayController.showUntilProgramTitleReady()
                        controller.channelDown()
                    }
                    RemoteCommand.OpenMiniEpg -> {
                        val currentProgramme = state.currentProgramOrNull()
                            ?.takeIf { !it.isEpgLookupPending && it.title.isNotBlank() }
                        miniEpgSelectedChannel = state.channel
                        miniEpgProgrammes = currentProgramme
                            ?.let { mapOf(state.channel.storageKey to it) }
                            ?: emptyMap()
                        miniEpgAttemptedChannels = currentProgramme
                            ?.let { setOf(state.channel.storageKey) }
                            ?: emptySet()
                        miniEpgArchiveStates = emptyMap()
                        miniEpgNowMs = System.currentTimeMillis()
                        miniEpgVisible = true
                        seekPreviewMs = null
                        scrubPositionMs = null
                        overlayController.hide()
                    }
                    RemoteCommand.MiniEpgUp -> {
                        miniEpgSelectedChannel = adjacentMiniEpgChannel(
                            channels = TvChannel.entries,
                            selectedChannel = miniEpgSelectedChannel ?: state.channel,
                            direction = -1,
                        )
                    }
                    RemoteCommand.MiniEpgDown -> {
                        miniEpgSelectedChannel = adjacentMiniEpgChannel(
                            channels = TvChannel.entries,
                            selectedChannel = miniEpgSelectedChannel ?: state.channel,
                            direction = 1,
                        )
                    }
                    RemoteCommand.MiniEpgPreviousProgramme -> {
                        val selected = miniEpgSelectedChannel ?: state.channel
                        miniEpgProgrammes[selected.storageKey]
                            ?.let(::previousProgrammeLookupTime)
                            ?.let { lookupMs -> loadMiniEpgProgramme(selected, lookupMs) }
                    }
                    RemoteCommand.MiniEpgNextProgramme -> {
                        val selected = miniEpgSelectedChannel ?: state.channel
                        miniEpgProgrammes[selected.storageKey]
                            ?.let(::nextProgrammeLookupTime)
                            ?.takeIf { it <= miniEpgNowMs }
                            ?.let { lookupMs -> loadMiniEpgProgramme(selected, lookupMs) }
                    }
                    RemoteCommand.SelectMiniEpgChannel -> {
                        val selected = miniEpgSelectedChannel ?: state.channel
                        val selectedProgram = miniEpgProgrammes[selected.storageKey]
                        val knownArchiveState = selectedProgram?.let { programme ->
                            miniEpgArchiveStates[miniEpgProgrammeKey(selected, programme)]
                        }
                        val archiveState = selectedProgram?.let { programme ->
                            miniEpgArchiveState(
                                channel = selected,
                                programme = programme,
                                nowMs = System.currentTimeMillis(),
                                available = when (knownArchiveState) {
                                    MiniEpgArchiveState.AVAILABLE -> true
                                    MiniEpgArchiveState.UNAVAILABLE -> false
                                    else -> null
                                },
                            )
                        } ?: MiniEpgArchiveState.NOT_APPLICABLE
                        when (
                            miniEpgSelectionAction(
                                programme = selectedProgram,
                                nowMs = System.currentTimeMillis(),
                                archiveState = archiveState,
                            )
                        ) {
                            MiniEpgSelectionAction.PLAY_ARCHIVE -> {
                                miniEpgVisible = false
                                miniEpgSelectedChannel = null
                                overlayController.showUntilProgramTitleReady()
                                controller.playArchive(selected, requireNotNull(selectedProgram))
                            }
                            MiniEpgSelectionAction.SELECT_LIVE -> {
                                miniEpgVisible = false
                                miniEpgSelectedChannel = null
                                overlayController.showUntilProgramTitleReady()
                                controller.selectChannel(selected)
                            }
                            MiniEpgSelectionAction.IGNORE -> return@onPreviewKeyEvent true
                        }
                    }
                    RemoteCommand.CloseMiniEpg -> {
                        miniEpgVisible = false
                        miniEpgSelectedChannel = null
                    }
                    RemoteCommand.ShowOverlay -> {
                        focusedControl = FocusedControl.TIMELINE
                    }
                    RemoteCommand.SeekBack -> {
                        seekPreviewMs = controller.seekBack()
                        controller.refreshPlaybackSnapshot()
                    }
                    RemoteCommand.SeekForward -> {
                        seekPreviewMs = controller.seekForward()
                        controller.refreshPlaybackSnapshot()
                    }
                    RemoteCommand.TogglePlayback -> {
                        controller.togglePlayback()
                    }
                    RemoteCommand.GoLive -> {
                        seekPreviewMs = null
                        scrubPositionMs = null
                        controller.goLive()
                        controller.refreshPlaybackSnapshot()
                    }
                    RemoteCommand.FocusPlayPause -> {
                        focusedControl = FocusedControl.PLAY_PAUSE
                    }
                    RemoteCommand.FocusLive -> {
                        focusedControl = FocusedControl.LIVE
                    }
                    RemoteCommand.FocusTimeline -> {
                        focusedControl = FocusedControl.TIMELINE
                    }
                    RemoteCommand.HideOverlay -> {
                        seekPreviewMs = null
                        scrubPositionMs = null
                        overlayController.hide()
                    }
                    RemoteCommand.Exit -> onExit()
                    RemoteCommand.Ignore -> return@onPreviewKeyEvent false
                }
                command.overlayTimeoutMs()?.let(overlayController::show)
                true
            }
            .focusable(),
    ) {
        AndroidView(
            factory = { context ->
                PlayerView(context).apply {
                    useController = false
                    resizeMode = AspectRatioFrameLayout.RESIZE_MODE_FIT
                    keepScreenOn = true
                    setKeepContentOnPlayerReset(true)
                    isFocusable = false
                    isFocusableInTouchMode = false
                    this.player = player
                }
            },
            update = { view -> view.player = player },
            modifier = Modifier.fillMaxSize(),
        )

        PlayerStateLayer(
            state = state,
            streamHost = streamHost,
            overlayVisible = overlayVisible,
            focusedControl = focusedControl,
            formatTime = { millis -> timeFormat.format(Date(millis)) },
            onSaveMarkizaCredentials = onSaveMarkizaCredentials,
            modifier = Modifier.align(Alignment.Center),
            seekPreviewMs = seekPreviewMs,
            seekPreviewPositionMs = scrubPositionMs,
            formatSeekTime = { millis -> seekTimeFormat.format(Date(millis)) },
        )

        if (miniEpgVisible) {
            MiniEpgOverlay(
                rows = buildMiniEpgRows(
                    channels = TvChannel.entries,
                    currentChannel = state.channel,
                    selectedChannel = miniEpgSelectedChannel ?: state.channel,
                    programmes = miniEpgProgrammes,
                    nowMs = miniEpgNowMs,
                    archiveStates = miniEpgArchiveStates,
                ),
            )
        }

        numericDigits?.let { digits ->
            NumericChannelIndicator(
                digits = digits,
                modifier = Modifier
                    .align(Alignment.TopEnd)
                    .padding(top = 88.dp, end = 36.dp),
            )
        }
    }
}

@Composable
internal fun NumericChannelIndicator(
    digits: String,
    modifier: Modifier = Modifier,
) {
    Text(
        text = digits,
        modifier = modifier
            .testTag("numeric-channel-indicator")
            .background(Color.Black.copy(alpha = 0.72f), RoundedCornerShape(12.dp))
            .padding(horizontal = 18.dp, vertical = 9.dp),
        color = Color.White,
        fontSize = 24.sp,
        fontWeight = FontWeight.Bold,
    )
}

@Composable
internal fun PlayerStateLayer(
    state: PlayerUiState,
    overlayVisible: Boolean,
    focusedControl: FocusedControl,
    formatTime: (Long) -> String,
    modifier: Modifier = Modifier,
    streamHost: String? = null,
    onSaveMarkizaCredentials: (String, String) -> Unit = { _, _ -> },
    seekPreviewMs: Long? = null,
    seekPreviewPositionMs: Long? = null,
    formatSeekTime: (Long) -> String = formatTime,
) {
    when (val current = state) {
        is PlayerUiState.Resolving -> StateOverlay(
            channel = current.channel,
            program = current.program,
            streamHost = streamHost,
            statusText = "Prepínam…",
            focusedControl = focusedControl,
            formatTime = formatTime,
            modifier = modifier,
        )

        is PlayerUiState.Preparing -> StateOverlay(
            channel = current.channel,
            program = current.program,
            streamHost = streamHost,
            statusText = "Prepínam…",
            focusedControl = focusedControl,
            formatTime = formatTime,
            modifier = modifier,
        )

        is PlayerUiState.Ready -> if (overlayVisible) {
            PlayerOverlay(
                model = PlayerOverlayModel.from(current, System.currentTimeMillis(), streamHost),
                focusedControl = focusedControl,
                timelineFocused = focusedControl == FocusedControl.TIMELINE,
                formatTime = formatTime,
                modifier = modifier,
                seekPreviewMs = seekPreviewMs,
                seekPreviewPositionMs = seekPreviewPositionMs,
                formatSeekTime = formatSeekTime,
            )
        }

        is PlayerUiState.Unavailable -> StateOverlay(
            channel = current.channel,
            program = current.program,
            streamHost = streamHost,
            statusText = current.nextRetryAtMs?.let { "Program nie je dostupný online · obnovím o ${formatTime(it)}" }
                ?: "Program nie je dostupný online",
            focusedControl = focusedControl,
            formatTime = formatTime,
            modifier = modifier,
        )

        is PlayerUiState.CredentialsRequired -> MarkizaCredentialsPanel(
            onSave = onSaveMarkizaCredentials,
            modifier = modifier,
        )

        is PlayerUiState.Error -> StateOverlay(
            channel = current.channel,
            program = current.program,
            streamHost = streamHost,
            statusText = current.nextRetryAtMs?.let { "Obnovím vysielanie o ${formatTime(it)}" }
                ?: current.message,
            focusedControl = focusedControl,
            formatTime = formatTime,
            modifier = modifier,
        )
    }
}

@Composable
private fun StateOverlay(
    channel: TvChannel,
    program: ProgramMetadata?,
    streamHost: String?,
    statusText: String,
    focusedControl: FocusedControl,
    formatTime: (Long) -> String,
    modifier: Modifier,
) {
    PlayerOverlay(
        model = PlayerOverlayModel.from(
            channel = channel,
            program = program,
            playback = null,
            streamHost = streamHost,
            statusText = statusText,
            nowMs = System.currentTimeMillis(),
        ),
        focusedControl = focusedControl,
        timelineFocused = false,
        formatTime = formatTime,
        modifier = modifier,
    )
}

@Composable
private fun MarkizaCredentialsPanel(
    onSave: (String, String) -> Unit,
    modifier: Modifier = Modifier,
) {
    val email = rememberTextFieldState()
    val password = rememberTextFieldState()
    Column(
        modifier = modifier
            .fillMaxWidth(0.68f)
            .background(Color(0xE6111114), RoundedCornerShape(22.dp))
            .padding(32.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        Text("MARKÍZA", color = Color.White.copy(alpha = 0.68f), fontWeight = FontWeight.Bold)
        Text("Prihlásenie k bezplatnému účtu", color = Color.White, fontSize = 22.sp)
        OutlinedTextField(
            state = email,
            label = { Text("E-mail") },
            lineLimits = TextFieldLineLimits.SingleLine,
        )
        OutlinedSecureTextField(
            state = password,
            label = { Text("Heslo") },
        )
        Button(
            onClick = { onSave(email.text.toString(), password.text.toString()) },
            enabled = email.text.isNotBlank() && password.text.isNotBlank(),
        ) {
            Text("Prihlásiť sa")
        }
    }
}

private fun PlayerUiState.hasResolvedProgramTitle(): Boolean = when (this) {
    is PlayerUiState.Preparing -> program?.let { !it.isEpgLookupPending && it.title.isNotBlank() } == true
    is PlayerUiState.Ready -> !program.isEpgLookupPending && program.title.isNotBlank()
    else -> false
}

private fun PlayerUiState.currentProgramOrNull(): ProgramMetadata? = when (this) {
    is PlayerUiState.Resolving -> program
    is PlayerUiState.Preparing -> program
    is PlayerUiState.Ready -> program
    is PlayerUiState.Unavailable -> program
    is PlayerUiState.Error -> program
    is PlayerUiState.CredentialsRequired -> null
}

internal fun shouldHandleRemoteKeyEvent(
    action: Int,
    keyCode: Int,
    miniEpgVisible: Boolean,
    overlayVisible: Boolean = true,
    focusedControl: FocusedControl = FocusedControl.TIMELINE,
): Boolean {
    val miniEpgVerticalNavigation = miniEpgVisible &&
        (keyCode == KeyEvent.KEYCODE_DPAD_UP || keyCode == KeyEvent.KEYCODE_DPAD_DOWN)
    val timelineScrub = !miniEpgVisible &&
        (keyCode == KeyEvent.KEYCODE_DPAD_LEFT || keyCode == KeyEvent.KEYCODE_DPAD_RIGHT) &&
        (!overlayVisible || focusedControl == FocusedControl.TIMELINE)

    return when {
        miniEpgVerticalNavigation -> action == KeyEvent.ACTION_DOWN
        timelineScrub -> action == KeyEvent.ACTION_DOWN || action == KeyEvent.ACTION_UP
        else -> action == KeyEvent.ACTION_UP
    }
}

internal fun seekScrubStepMs(repeatCount: Int): Long = when {
    repeatCount >= 50 -> 300_000L
    repeatCount >= 20 -> 60_000L
    repeatCount >= 6 -> 30_000L
    else -> 10_000L
}

private fun Int.isCenterKey(): Boolean =
    this == KeyEvent.KEYCODE_DPAD_CENTER ||
        this == KeyEvent.KEYCODE_ENTER ||
        this == KeyEvent.KEYCODE_NUMPAD_ENTER

private const val SEEK_PREVIEW_DURATION_MS = 1_800L