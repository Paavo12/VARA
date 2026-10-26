import type { CapacitorConfig } from '@capacitor/cli';

// The app ID reverses the placeholder domain fuber.app. It cannot change after the first
// store release, so confirm the domain before publishing.
const config: CapacitorConfig = {
	appId: 'app.fuber.rider',
	appName: 'FUber',
	webDir: 'build',
	android: {
		allowMixedContent: false
	}
};

export default config;
