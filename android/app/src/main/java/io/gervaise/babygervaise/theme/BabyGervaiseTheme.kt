package io.gervaise.babygervaise.theme

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Shapes
import androidx.compose.material3.Typography
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

private val WarmColorScheme = lightColorScheme(
    primary = Color(0xFF2E2017),
    onPrimary = Color(0xFFF7F1EA),
    primaryContainer = Color(0xFFF0DAB6),
    onPrimaryContainer = Color(0xFF2E2017),
    secondary = Color(0xFF866246),
    onSecondary = Color(0xFFFFF7EF),
    secondaryContainer = Color(0xFFEFE4D2),
    onSecondaryContainer = Color(0xFF2E2017),
    tertiary = Color(0xFFD4A15A),
    onTertiary = Color(0xFF2E2017),
    background = Color(0xFFF6F0E8),
    onBackground = Color(0xFF2E2017),
    surface = Color(0xFFFFFCF7),
    onSurface = Color(0xFF2E2017),
    surfaceVariant = Color(0xFFEFE4D2),
    onSurfaceVariant = Color(0xFF654830),
    outline = Color(0x1F2E2017),
)

private val BabyGervaiseShapes = Shapes(
    extraSmall = androidx.compose.foundation.shape.RoundedCornerShape(12.dp),
    small = androidx.compose.foundation.shape.RoundedCornerShape(18.dp),
    medium = androidx.compose.foundation.shape.RoundedCornerShape(24.dp),
    large = androidx.compose.foundation.shape.RoundedCornerShape(28.dp),
    extraLarge = androidx.compose.foundation.shape.RoundedCornerShape(32.dp),
)

private val BabyGervaiseTypography = Typography(
    headlineLarge = TextStyle(
        fontFamily = FontFamily.SansSerif,
        fontSize = 40.sp,
        lineHeight = 38.sp,
        letterSpacing = (-1.2).sp,
    ),
    headlineSmall = TextStyle(
        fontFamily = FontFamily.SansSerif,
        fontSize = 24.sp,
        lineHeight = 28.sp,
        letterSpacing = (-0.4).sp,
    ),
    titleMedium = TextStyle(
        fontFamily = FontFamily.SansSerif,
        fontSize = 18.sp,
        lineHeight = 24.sp,
    ),
    bodyLarge = TextStyle(
        fontFamily = FontFamily.SansSerif,
        fontSize = 16.sp,
        lineHeight = 24.sp,
    ),
    bodyMedium = TextStyle(
        fontFamily = FontFamily.SansSerif,
        fontSize = 14.sp,
        lineHeight = 20.sp,
    ),
    labelLarge = TextStyle(
        fontFamily = FontFamily.SansSerif,
        fontSize = 14.sp,
        lineHeight = 18.sp,
    ),
    labelSmall = TextStyle(
        fontFamily = FontFamily.SansSerif,
        fontSize = 11.sp,
        lineHeight = 14.sp,
        letterSpacing = 1.6.sp,
    ),
)

@Composable
fun BabyGervaiseTheme(
    content: @Composable () -> Unit,
) {
    MaterialTheme(
        colorScheme = WarmColorScheme,
        shapes = BabyGervaiseShapes,
        typography = BabyGervaiseTypography,
        content = content,
    )
}
