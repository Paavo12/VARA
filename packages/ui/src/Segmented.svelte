<!--
	A row of mutually exclusive choices (design brief §6: For me / Someone else, Now / Schedule).
	A radio group underneath, so it works with a keyboard and screen readers.
-->
<script lang="ts" generics="T extends string">
	interface Props {
		label: string;
		options: { value: T; label: string; disabled?: boolean }[];
		value: T;
	}

	let { label, options, value = $bindable() }: Props = $props();
	const name = `seg-${Math.random().toString(36).slice(2, 9)}`;
</script>

<fieldset class="seg">
	<legend>{label}</legend>
	<div class="row">
		{#each options as o (o.value)}
			<label class:on={value === o.value} class:off={o.disabled}>
				<input type="radio" {name} value={o.value} bind:group={value} disabled={o.disabled} />
				{o.label}
			</label>
		{/each}
	</div>
</fieldset>

<style>
	.seg {
		border: none;
		margin: 0;
		padding: 0;
		display: grid;
		gap: var(--space-1);
	}
	legend {
		padding: 0;
		margin-bottom: var(--space-1);
		font-weight: 600;
		font-size: var(--text-sm);
		color: var(--color-text-secondary);
	}
	.row {
		display: flex;
		gap: var(--space-1);
		padding: var(--space-1);
		border-radius: var(--radius-pill);
		background: var(--color-surface-tinted);
	}
	label {
		flex: 1;
		display: grid;
		place-items: center;
		min-height: var(--touch-target);
		border-radius: var(--radius-pill);
		font-weight: 600;
		cursor: pointer;
		color: var(--color-text-secondary);
	}
	label.on {
		background: var(--color-surface);
		color: var(--color-brand);
		box-shadow: var(--shadow-sm);
	}
	label.off {
		opacity: 0.5;
		cursor: not-allowed;
	}
	label:has(input:focus-visible) {
		outline: 3px solid var(--color-focus-ring);
		outline-offset: 2px;
	}
	input {
		position: absolute;
		opacity: 0;
		pointer-events: none;
	}
</style>
