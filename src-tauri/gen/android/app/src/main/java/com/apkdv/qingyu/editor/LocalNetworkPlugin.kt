package com.apkdv.qingyu.editor

import android.Manifest
import android.app.Activity
import android.os.Build
import androidx.annotation.Keep
import app.tauri.PermissionState
import app.tauri.annotation.Command
import app.tauri.annotation.Permission
import app.tauri.annotation.PermissionCallback
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.Plugin

@Keep
@TauriPlugin(permissions = [Permission(alias = "localNetwork", strings = [Manifest.permission.ACCESS_LOCAL_NETWORK])])
class LocalNetworkPlugin(activity: Activity) : Plugin(activity) {
  @Command
  fun access(invoke: Invoke) {
    if (Build.VERSION.SDK_INT < 37) {
      invoke.resolve()
    } else if (invoke.getArgs().optBoolean("request") && getPermissionState("localNetwork") != PermissionState.GRANTED) {
      requestPermissionForAlias("localNetwork", invoke, "localNetworkResult")
    } else {
      localNetworkResult(invoke)
    }
  }

  @PermissionCallback
  fun localNetworkResult(invoke: Invoke) {
    invoke.resolveObject(getPermissionState("localNetwork") == PermissionState.GRANTED)
  }
}
