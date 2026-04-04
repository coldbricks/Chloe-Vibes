# Keep BLE GATT callback methods accessed via reflection
-keep class * extends android.bluetooth.BluetoothGattCallback { *; }
-keep class com.ashairfoil.chloevibes.device.BleDeviceManager$* { *; }

# Keep Compose runtime
-keep class androidx.compose.** { *; }
