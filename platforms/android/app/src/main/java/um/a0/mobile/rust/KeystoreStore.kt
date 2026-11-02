package um.a0.mobile.rust

import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyPermanentlyInvalidatedException
import android.security.keystore.KeyProperties
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * Kotlin-side implementation of the UniFFI SecretStore callback interface,
 * backed by the Android Keystore (feature-parity.md section 4: Keystore seam
 * `a0-platform::secure_store`).
 *
 * - AES/GCM 256-bit keys inside AndroidKeyStore (hardware-backed when the
 *   device supports it).
 * - StrongBox attempted first, silently falling back to TEE/software backing.
 * - TODO(bindings): conform to the generated callback protocol once the
 *   um.a0.platform bindings are generated; the Rust core invokes this object
 *   through the UniFFI callback trampoline (JNI thread of the caller).
 */
object KeystoreStore {

    private const val ANDROID_KEYSTORE = "AndroidKeyStore"
    private const val KEY_ALIAS = "a0_platform_secret_store"
    private const val GCM_TAG_LENGTH_BITS = 128
    private const val GCM_IV_LENGTH_BYTES = 12

    private fun obtainKey(): SecretKey {
        val keyStore = KeyStore.getInstance(ANDROID_KEYSTORE).apply { load(null) }
        (keyStore.getKey(KEY_ALIAS, null) as? SecretKey)?.let { return it }

        val generator = KeyGenerator.getInstance(
            KeyProperties.KEY_ALGORITHM_AES,
            ANDROID_KEYSTORE
        )
        val spec = KeyGenParameterSpec.Builder(
            KEY_ALIAS,
            KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT
        )
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
            .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
            .setKeySize(256)
            .setRandomizedEncryptionRequired(true)
        // StrongBox where available (Pixel-class devices); fallback handled
        // by catching the setup failure below.
        try {
            generator.init(spec
                .setIsStrongBoxBacked(true)
                .build())
            return generator.generateKey()
        } catch (e: Exception) {
            // StrongBox unavailable: regenerate without the flag.
        }
        generator.init(spec.build())
        return generator.generateKey()
    }

    fun set(account: String, secret: String) {
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, obtainKey())
        val iv = cipher.iv
        val ciphertext = cipher.doFinal(secret.toByteArray(Charsets.UTF_8))
        // TODO(bindings): persist ciphertext + IV via the Rust a0-storage seam
        // (EncryptedSharedPreferences/DataStore per feature-parity.md section 4);
        // the key itself never leaves the Keystore.
        val ivB64 = android.util.Base64.encodeToString(iv, android.util.Base64.NO_WRAP)
        val ctB64 = android.util.Base64.encodeToString(ciphertext, android.util.Base64.NO_WRAP)
        android.util.Log.d("KeystoreStore", "stored account=$account (binding persistence pending)")
        // Temporary in-memory overlay until the storage seam lands:
        memory[account] = ivB64 + ":" + ctB64
    }

    fun get(account: String): String? {
        val combined = memory[account] ?: return null
        return try {
            val parts = combined.split(":")
            val iv = android.util.Base64.decode(parts[0], android.util.Base64.NO_WRAP)
            val ct = android.util.Base64.decode(parts[1], android.util.Base64.NO_WRAP)
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(Cipher.DECRYPT_MODE, obtainKey(), GCMParameterSpec(GCM_TAG_LENGTH_BITS, iv))
            String(cipher.doFinal(ct), Charsets.UTF_8)
        } catch (e: KeyPermanentlyInvalidatedException) {
            null
        } catch (e: Exception) {
            null
        }
    }

    fun delete(account: String) {
        memory.remove(account)
        // TODO(bindings): also remove the persisted ciphertext record.
    }

    // Temporary overlay until the a0-storage persistence seam lands.
    private val memory = mutableMapOf<String, String>()
}
