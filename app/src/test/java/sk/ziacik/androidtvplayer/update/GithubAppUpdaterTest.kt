package sk.ziacik.androidtvplayer.update

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class GithubAppUpdaterTest {
    @Test
    fun recognizesOfficialFdroidInstaller() {
        assertTrue(isFdroidInstallerPackage("org.fdroid.fdroid"))
    }

    @Test
    fun recognizesFdroidBasicInstaller() {
        assertTrue(isFdroidInstallerPackage("org.fdroid.basic"))
    }

    @Test
    fun keepsSelfUpdaterForSideloadsAndOtherInstallers() {
        assertFalse(isFdroidInstallerPackage(null))
        assertFalse(isFdroidInstallerPackage("com.android.packageinstaller"))
        assertFalse(isFdroidInstallerPackage("com.google.android.packageinstaller"))
    }
}
