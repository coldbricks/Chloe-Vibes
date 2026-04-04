// ==========================================================================
// Theme.kt -- Jetpack Compose dark theme
// Matches the desktop app's purple/teal/pink palette
// ==========================================================================

package com.ashairfoil.chloevibes.ui

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

// ---------------------------------------------------------------------------
// Brand colors
// ---------------------------------------------------------------------------

object ChloeColors {
    val Purple = Color(0xFF7C5CFC)
    val PurpleLight = Color(0xFF9B82FF)
    val PurpleDark = Color(0xFF5A3DD4)
    val Teal = Color(0xFF30D8D0)
    val TealLight = Color(0xFF5FFFEB)
    val TealDark = Color(0xFF00A89E)
    val Pink = Color(0xFFE060A0)
    val PinkLight = Color(0xFFFF8CC4)
    val PinkDark = Color(0xFFB33878)
    val Amber = Color(0xFFFFB74D)

    // Surface/background
    val Background = Color(0xFF0D0D1A)
    val Surface = Color(0xFF1A1A2E)
    val SurfaceVariant = Color(0xFF252540)
    val SurfaceBright = Color(0xFF30304D)
    val OnBackground = Color(0xFFE8E8F0)
    val OnSurface = Color(0xFFD0D0E0)
    val OnSurfaceDim = Color(0xFF8888AA)

    // ADSR phase colors (matching desktop UI)
    val Attack = Teal
    val Decay = Purple
    val Sustain = Amber
    val Release = Color(0xFFE05050)

    // Status
    val GateOpen = Teal
    val GateClosed = Color(0xFF555577)
    val Connected = Teal
    val Disconnected = Color(0xFF666688)
    val Error = Color(0xFFFF4444)
}

// ---------------------------------------------------------------------------
// Material3 dark color scheme
// ---------------------------------------------------------------------------

private val DarkColorScheme = darkColorScheme(
    primary = ChloeColors.Purple,
    onPrimary = Color.White,
    primaryContainer = ChloeColors.PurpleDark,
    onPrimaryContainer = ChloeColors.PurpleLight,

    secondary = ChloeColors.Teal,
    onSecondary = Color.Black,
    secondaryContainer = ChloeColors.TealDark,
    onSecondaryContainer = ChloeColors.TealLight,

    tertiary = ChloeColors.Pink,
    onTertiary = Color.White,
    tertiaryContainer = ChloeColors.PinkDark,
    onTertiaryContainer = ChloeColors.PinkLight,

    background = ChloeColors.Background,
    onBackground = ChloeColors.OnBackground,

    surface = ChloeColors.Surface,
    onSurface = ChloeColors.OnSurface,
    surfaceVariant = ChloeColors.SurfaceVariant,
    onSurfaceVariant = ChloeColors.OnSurfaceDim,

    error = ChloeColors.Error,
    onError = Color.White,

    outline = Color(0xFF444466),
    outlineVariant = Color(0xFF333355)
)

// ---------------------------------------------------------------------------
// Theme composable
// ---------------------------------------------------------------------------

@Composable
fun ChloeVibesTheme(content: @Composable () -> Unit) {
    MaterialTheme(
        colorScheme = DarkColorScheme,
        typography = Typography(),
        content = content
    )
}
