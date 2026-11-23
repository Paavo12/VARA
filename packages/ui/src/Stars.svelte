<!--
	1–5 star rating input (design brief §6 "Rating stars"). A radio group, so it works with the
	keyboard (arrow keys) and screen readers ("4 of 5 stars").
-->
<script lang="ts">
	interface Props {
		value?: number;
		label: string;
	}

	let { value = $bindable(0), label }: Props = $props();
	const name = `stars-${Math.random().toString(36).slice(2, 9)}`;
</script>

<fieldset class="stars">
	<legend>{label}</legend>
	<div class="row">
		{#each [1, 2, 3, 4, 5] as n (n)}
			<label class:on={n <= value}>
				<input type="radio" {name} value={n} bind:group={value} />
				<span aria-hidden="true">{n <= value ? '★' : '☆'}</span>
				<span class="sr-only">{n} of 5 stars</span>
			</label>
		{/each}
	</div>
</fieldset>

<style>
	.stars {
		border: 0;
		margin: 0;
		padding: 0;
		display: grid;
		gap: var(--space-2);
		justify-items: center;
	}
	legend {
		margin-bottom: var(--space-2);
		font-weight: 700;
		text-align: center;
	}
	.row {
		display: flex;
		gap: var(--space-1);
	}
	label {
		position: relative;
		display: grid;
		place-items: center;
		width: var(--touch-target-driver);
		height: var(--touch-target-driver);
		font-size: 2.25rem;
		line-height: 1;
		/* Outline stars in muted text and filled ones in --color-rating: both 3:1 or more, so the
		   rating shows without relying on a pale fill (WCAG 1.4.11). */
		color: var(--color-text-muted);
		cursor: pointer;
	}
	label.on {
		color: var(--color-rating);
	}
	input {
		position: absolute;
		opacity: 0;
		inset: 0;
		margin: 0;
		cursor: pointer;
	}
	label:has(input:focus-visible) {
		outline: 3px solid var(--color-focus-ring);
		border-radius: var(--radius-sm);
	}
</style>
