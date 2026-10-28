<!--
	A vehicle plate in the Namibian style (design brief §6), e.g. N 6152 W: black on white with
	a thin border, so riders can match it to the car at a glance.
-->
<script lang="ts">
	interface Props {
		plate: string;
	}

	let { plate }: Props = $props();
</script>

<span class="plate" aria-label="Number plate {plate}">{plate}</span>

<style>
	.plate {
		display: inline-block;
		padding: 2px var(--space-2);
		border: 2px solid #1d1724;
		border-radius: 4px;
		background: #ffffff;
		color: #1d1724;
		font-weight: 800;
		letter-spacing: 0.08em;
		font-variant-numeric: tabular-nums;
		white-space: nowrap;
	}
</style>
