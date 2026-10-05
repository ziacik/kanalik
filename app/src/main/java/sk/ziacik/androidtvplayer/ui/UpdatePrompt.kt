package sk.ziacik.androidtvplayer.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import sk.ziacik.androidtvplayer.update.AppUpdateState

@Composable
fun UpdatePrompt(
    state: AppUpdateState,
    onUpdate: () -> Unit,
    onLater: () -> Unit,
    modifier: Modifier = Modifier,
) {
    if (state is AppUpdateState.Hidden) return

    val focusRequester = remember { FocusRequester() }
    LaunchedEffect(state) {
        if (state is AppUpdateState.Available || state is AppUpdateState.Error) {
            focusRequester.requestFocus()
        }
    }

    val info = when (state) {
        is AppUpdateState.Available -> state.info
        is AppUpdateState.Downloading -> state.info
        is AppUpdateState.Error -> state.info
        AppUpdateState.Hidden -> return
    }

    Column(
        modifier = modifier
            .fillMaxWidth(0.62f)
            .background(Color(0xF018181C), RoundedCornerShape(22.dp))
            .padding(30.dp),
        horizontalAlignment = Alignment.Start,
        verticalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        Text(
            text = "NOVÁ VERZIA",
            color = Color.White.copy(alpha = 0.68f),
            fontWeight = FontWeight.Bold,
        )
        Text(
            text = "Kanálik ${info.versionName}",
            color = Color.White,
            fontSize = 24.sp,
            fontWeight = FontWeight.Bold,
        )

        when (state) {
            is AppUpdateState.Downloading -> {
                Text("Sťahujem a overujem aktualizáciu…", color = Color.White)
            }
            is AppUpdateState.Error -> {
                Text(state.message, color = Color.White)
            }
            else -> {
                Text(
                    "Aktualizácia sa stiahne z GitHub Releases a Android si vypýta potvrdenie inštalácie.",
                    color = Color.White.copy(alpha = 0.82f),
                )
            }
        }

        if (state !is AppUpdateState.Downloading) {
            Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                Button(
                    onClick = onUpdate,
                    modifier = Modifier.focusRequester(focusRequester),
                ) {
                    Text(if (state is AppUpdateState.Error) "Skúsiť znova" else "Aktualizovať")
                }
                Button(onClick = onLater) {
                    Text("Neskôr")
                }
            }
        }
    }
}
