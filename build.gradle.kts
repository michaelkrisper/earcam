plugins {
    id("com.android.application") version "9.4.1" apply false
    // Not applied (AGP 9 compiles Kotlin itself); only pins the newer compiler it uses.
    id("org.jetbrains.kotlin.android") version "2.4.20" apply false
}
