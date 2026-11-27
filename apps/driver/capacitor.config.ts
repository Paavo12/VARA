import type { CapacitorConfig } from '@capacitor/cli';

// The app ID keeps the old name until the client confirms the domain. It cannot change after the first
// store release, so confirm the domain before publishing.
const config: CapacitorConfig = {
	appId: 'app.fuber.driver',
	appName: 'VARA Driver',
	webDir: 'build',
	android: {
		allowMixedContent: false
	},
	plugins: {
		// The app shows the live state itself while open, so no banner on top of it (iOS).
		FirebaseMessaging: { presentationOptions: [] }
	},
	experimental: {
		// Avoids a SwiftPM package identity clash (plugin README).
		ios: { spm: { packageOptions: { '@capacitor-firebase/messaging': { symlink: true } } } }
	},
	// Local development on an emulator only (`CAP_LOCAL_API=1 npx cap sync`): serve the app over
	// http so it may call the API on http://127.0.0.1:8080 (reached through `adb reverse`).
	// Release builds never set this.
	...(process.env.CAP_LOCAL_API === '1' && { server: { androidScheme: 'http', cleartext: true } })
};

export default config;
