import type { CapacitorConfig } from '@capacitor/cli';

// The app ID reverses the placeholder domain fuber.app. It cannot change after the first
// store release, so confirm the domain before publishing.
const config: CapacitorConfig = {
	appId: 'app.fuber.rider',
	appName: 'FUber',
	webDir: 'build',
	android: {
		allowMixedContent: false
	},
	// Local development on an emulator only (`CAP_LOCAL_API=1 npx cap sync`): serve the app over
	// http so it may call the API on http://127.0.0.1:8080 (reached through `adb reverse`).
	// Release builds never set this.
	...(process.env.CAP_LOCAL_API === '1' && { server: { androidScheme: 'http', cleartext: true } })
};

export default config;
