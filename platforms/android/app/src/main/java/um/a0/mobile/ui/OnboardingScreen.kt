package um.a0.mobile.ui

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp

/**
 * First-run onboarding: server origin entry, QR scan review and credentials.
 * Mirrors the iOS OnboardingView flow (Phase 1 parity, feature-parity.md 2.1).
 */
private enum class Step { Origin, QrReview, Credentials }

@Composable
fun OnboardingScreen(modifier: Modifier = Modifier) {
    var step by remember { mutableStateOf(Step.Origin) }
    var origin by remember { mutableStateOf("") }
    var username by remember { mutableStateOf("") }
    var password by remember { mutableStateOf("") }
    var reviewedQr by remember { mutableStateOf(false) }

    Column(
        modifier = modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(16.dp)
    ) {
        Text("Welcome to Agent Zero", style = MaterialTheme.typography.headlineSmall)
        Spacer(Modifier.height(16.dp))

        when (step) {
            Step.Origin -> {
                OutlinedTextField(
                    value = origin,
                    onValueChange = { origin = it },
                    label = { Text("Server origin") },
                    placeholder = { Text("https://agent.example.com") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth()
                )
                // TODO(bindings): camera QR scan routes to a0-platform::pickers.
                Button(
                    onClick = { step = Step.QrReview },
                    enabled = origin.isNotBlank(),
                    modifier = Modifier.fillMaxWidth()
                ) { Text("Continue") }
            }
            Step.QrReview -> {
                Text("Scanned value:", style = MaterialTheme.typography.labelMedium)
                Text(origin.ifBlank { "(no code scanned)" }, style = MaterialTheme.typography.bodySmall)
                androidx.compose.material3.Checkbox(
                    checked = reviewedQr,
                    onCheckedChange = { reviewedQr = it }
                )
                Text("Code verified", style = MaterialTheme.typography.bodySmall)
                Button(
                    onClick = { step = Step.Credentials },
                    enabled = reviewedQr,
                    modifier = Modifier.fillMaxWidth()
                ) { Text("Continue") }
            }
            Step.Credentials -> {
                OutlinedTextField(
                    value = username,
                    onValueChange = { username = it },
                    label = { Text("Username") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth()
                )
                OutlinedTextField(
                    value = password,
                    onValueChange = { password = it },
                    label = { Text("Password") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth()
                )
                Button(
                    onClick = {
                        // TODO(bindings): persist credentials via KeystoreStore
                        // and call RustBridge.connect(origin, ...) first.
                    },
                    enabled = username.isNotBlank() && password.isNotBlank(),
                    modifier = Modifier.fillMaxWidth()
                ) { Text("Connect") }
            }
        }
    }
}
