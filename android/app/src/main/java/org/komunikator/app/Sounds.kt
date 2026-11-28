package org.komunikator.app

import android.content.Context
import android.media.AudioAttributes
import android.media.SoundPool

/**
 * Szept's own sounds inside the app, from tools/sounds.py: a message
 * arriving in the conversation on screen, a message leaving, money coming
 * in. Sounds for notifications go through their channel instead.
 */
class Sounds(context: Context) {
    private val pool = SoundPool.Builder()
        .setMaxStreams(3)
        .setAudioAttributes(
            AudioAttributes.Builder()
                .setUsage(AudioAttributes.USAGE_ASSISTANCE_SONIFICATION)
                .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
                .build(),
        )
        .build()

    /** Sound id → the pool's id, loaded once at start: they are tiny. */
    private val loaded: Map<String, Int> = SOUND_RESOURCES.mapValues { (_, resource) -> pool.load(context, resource, 1) }

    fun play(id: String, volume: Float = 0.7f) {
        val sample = loaded[id] ?: return
        pool.play(sample, volume, volume, 1, 0, 1f)
    }
}
