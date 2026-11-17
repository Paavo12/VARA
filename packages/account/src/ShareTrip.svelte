<!--
	Share trip (PRD S-2, App Flow §6.3), in either app from the driver accepting until drop-off.
	Each tap makes a fresh live link; she sends it with the phone's share sheet (or copies it
	where there is none) or texts it to her emergency contacts. Links stop working 30 minutes
	after the trip ends.
-->
<script lang="ts">
	import { Button, Notice } from '@fuber/ui';
	import { messageOf, useSession } from './context';

	interface Props {
		rideId: string;
	}

	let { rideId }: Props = $props();
	const session = useSession();

	let open = $state(false);
	let busy = $state(false);
	let error = $state('');
	let done = $state('');

	async function share(toContacts: boolean) {
		busy = true;
		error = '';
		done = '';
		const { data, error: e } = await session.api
			.POST('/v1/rides/{id}/share', {
				params: { path: { id: rideId } },
				body: { send_to_contacts: toContacts }
			})
			.catch(() => ({ data: undefined, error: undefined }));
		busy = false;
		if (!data) {
			error = messageOf(e, "We couldn't make a link. Check your connection and try again.");
			return;
		}
		if (toContacts) {
			done =
				data.contacts_texted === 0
					? 'You have no emergency contacts yet. Add them in your profile.'
					: data.contacts_texted === 1
						? 'We texted the link to your emergency contact.'
						: `We texted the link to your ${data.contacts_texted} emergency contacts.`;
			return;
		}
		if (navigator.share) {
			try {
				await navigator.share({ title: 'My VARA trip', text: data.text });
				open = false;
			} catch {
				// She closed the share sheet: nothing to do.
			}
			return;
		}
		try {
			await navigator.clipboard.writeText(data.text);
			done = 'Link copied. Paste it into a message.';
		} catch {
			done = data.url;
		}
	}
</script>

<Button variant="secondary" onclick={() => ((open = !open), (error = ''), (done = ''))}>
	Share trip
</Button>

{#if open}
	<section class="share" aria-label="Share trip">
		<p>
			Send a live link: they see the car, the plate and when you arrive, until 30 minutes after the
			trip.
		</p>
		<Button disabled={busy} onclick={() => share(false)}>Share link…</Button>
		<Button variant="secondary" disabled={busy} onclick={() => share(true)}>
			Send to my emergency contacts
		</Button>
		{#if error}<Notice tone="danger"><p>{error}</p></Notice>{/if}
		{#if done}<Notice tone="success"><p class="done">{done}</p></Notice>{/if}
	</section>
{/if}

<style>
	.share {
		display: flex;
		flex-direction: column;
		gap: var(--space-2);
		padding: var(--space-3);
		border: 1px solid var(--color-border);
		border-radius: var(--radius-md);
	}
	.share p {
		margin: 0;
	}
	.done {
		overflow-wrap: anywhere;
	}
</style>
