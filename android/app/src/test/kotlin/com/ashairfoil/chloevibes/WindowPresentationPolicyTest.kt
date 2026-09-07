package com.ashairfoil.chloevibes

import java.nio.file.Files
import java.nio.file.Path
import javax.xml.parsers.DocumentBuilderFactory
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test
import org.w3c.dom.Element

class WindowPresentationPolicyTest {
    @Test fun `Horizon panels prefer landscape with either supported system feature`() {
        for (features in listOf(
            setOf("horizonos.software.horizon_os"),
            setOf("oculus.hardware.standalone_vr"),
            setOf("horizonos.software.horizon_os", "oculus.hardware.standalone_vr"),
        )) {
            assertTrue(WindowPresentationPolicy.prefersLandscape(features::contains))
        }
    }

    @Test fun `phones tablets and unrelated VR features keep their existing orientation`() {
        for (features in listOf(
            emptySet(),
            setOf("android.hardware.touchscreen", "android.hardware.sensor.accelerometer"),
            setOf("android.software.leanback", "android.hardware.vr.high_performance"),
        )) {
            assertFalse(WindowPresentationPolicy.prefersLandscape(features::contains))
        }
    }

    @Test fun `manifest supplies a resizable landscape panel without globally locking phone orientation`() {
        val manifestPath = sequenceOf(
            Path.of("src/main/AndroidManifest.xml"),
            Path.of("app/src/main/AndroidManifest.xml"),
        ).first { Files.isRegularFile(it) }
        val factory = DocumentBuilderFactory.newInstance().apply {
            isNamespaceAware = true
            setFeature("http://apache.org/xml/features/disallow-doctype-decl", true)
        }
        val document = factory.newDocumentBuilder().parse(manifestPath.toFile())
        val activities = document.getElementsByTagName("activity")
        val android = "http://schemas.android.com/apk/res/android"
        val activity = (0 until activities.length)
            .map { activities.item(it) as Element }
            .single { it.getAttributeNS(android, "name") == ".MainActivity" }
        assertEquals("true", activity.getAttributeNS(android, "resizeableActivity"))
        assertFalse(activity.hasAttributeNS(android, "screenOrientation"))
        val layout = activity.getElementsByTagName("layout").item(0) as Element
        assertEquals("1280dp", layout.getAttributeNS(android, "defaultWidth"))
        assertEquals("720dp", layout.getAttributeNS(android, "defaultHeight"))
        val handled = activity.getAttributeNS(android, "configChanges").split("|")
        assertTrue(handled.containsAll(listOf("orientation", "screenSize", "smallestScreenSize")))
    }
}
