package com.ashairfoil.chloevibes.device

/** Small coalescing queue: rests survive a later beat; discovery survives audio. */
internal class BleCommandQueue {
    private var stop: String? = null
    private var intensity: String? = null
    private val control = ArrayDeque<String>()

    fun offer(command: String) {
        when {
            LovenseProtocol.isStopCommand(command) -> {
                stop = command
                intensity = null
            }
            command.startsWith("Vibrate") -> intensity = command
            command !in control && control.size < 8 -> control.addLast(command)
        }
    }

    fun retry(command: String) {
        when {
            LovenseProtocol.isStopCommand(command) -> stop = command
            command.startsWith("Vibrate") -> if (intensity == null && stop == null) intensity = command
            command !in control && control.size < 8 -> control.addFirst(command)
        }
    }

    fun peek(): String? = stop ?: control.firstOrNull() ?: intensity

    fun take(): String? {
        stop?.let { stop = null; return it }
        if (control.isNotEmpty()) return control.removeFirst()
        return intensity.also { intensity = null }
    }

    fun clear() {
        stop = null
        intensity = null
        control.clear()
    }
}
