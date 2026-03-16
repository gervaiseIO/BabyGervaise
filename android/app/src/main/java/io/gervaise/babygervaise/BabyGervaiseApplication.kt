package io.gervaise.babygervaise

import android.app.Application

class BabyGervaiseApplication : Application() {
    val runtime: BabyGervaiseRuntime by lazy(LazyThreadSafetyMode.NONE) {
        BabyGervaiseRuntime(this)
    }
}
