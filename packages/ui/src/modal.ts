/**
 * Keyboard and screen-reader focus for a modal dialog (WCAG 2.4.3 Focus Order, 2.1.2 No Keyboard
 * Trap): `<div role="alertdialog" aria-modal="true" use:modal>`.
 *
 * - Moves focus into the dialog when it opens: to the element marked `data-autofocus`, else the
 *   first control.
 * - Keeps Tab and Shift+Tab inside it, as aria-modal promises.
 * - When its content changes and the focused control is gone (a step of the SOS flow replaces its
 *   buttons), moves focus back into the dialog instead of leaving it on the page behind.
 * - Gives focus back to whatever had it before, when the dialog closes.
 */
const FOCUSABLE =
	'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

export function modal(node: HTMLElement) {
	const before = document.activeElement instanceof HTMLElement ? document.activeElement : null;

	const controls = () => [...node.querySelectorAll<HTMLElement>(FOCUSABLE)];
	const focusFirst = () => {
		const target = node.querySelector<HTMLElement>('[data-autofocus]') ?? controls()[0] ?? node;
		if (target === node && !node.hasAttribute('tabindex')) node.tabIndex = -1;
		target.focus();
	};

	function onkeydown(event: KeyboardEvent) {
		if (event.key !== 'Tab') return;
		const all = controls();
		if (all.length === 0) {
			event.preventDefault();
			return;
		}
		const first = all[0];
		const last = all[all.length - 1];
		if (event.shiftKey && document.activeElement === first) {
			event.preventDefault();
			last.focus();
		} else if (!event.shiftKey && document.activeElement === last) {
			event.preventDefault();
			first.focus();
		}
	}

	const observer = new MutationObserver(() => {
		if (!node.contains(document.activeElement)) focusFirst();
	});

	focusFirst();
	node.addEventListener('keydown', onkeydown);
	observer.observe(node, { childList: true, subtree: true });

	return {
		destroy() {
			observer.disconnect();
			node.removeEventListener('keydown', onkeydown);
			if (before?.isConnected) before.focus();
		}
	};
}
