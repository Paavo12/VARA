<!-- Test only: gives Chat a session the way the apps do, and exposes it to the test. -->
<script lang="ts">
	import type { Session } from '@fuber/api-client';
	import { untrack } from 'svelte';
	import Chat from '../Chat.svelte';
	import { provideSession } from '../context';

	let { session, rideId }: { session: Session; rideId: string } = $props();
	untrack(() => provideSession(session));
	let chat = $state<Chat>();

	export function component() {
		return chat!;
	}
</script>

<Chat bind:this={chat} {rideId} party="rider" name="Hilma" />
