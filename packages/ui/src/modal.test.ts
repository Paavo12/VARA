// @vitest-environment jsdom
import { afterEach, describe, expect, it } from 'vitest';
import { modal } from './modal';

const tick = () => new Promise((r) => setTimeout(r, 0));

function dialog(html: string) {
	const node = document.createElement('div');
	node.innerHTML = html;
	document.body.append(node);
	return node;
}

function tab(node: HTMLElement, shiftKey = false) {
	const event = new KeyboardEvent('keydown', { key: 'Tab', shiftKey, cancelable: true });
	node.dispatchEvent(event);
	return event.defaultPrevented;
}

afterEach(() => {
	document.body.innerHTML = '';
});

describe('modal', () => {
	it('focuses the data-autofocus control, else the first one', () => {
		const a = dialog('<button>One</button><button data-autofocus>Two</button>');
		modal(a);
		expect(document.activeElement?.textContent).toBe('Two');

		const b = dialog('<p>Text</p><a href="tel:10111">Call</a><button>Close</button>');
		modal(b);
		expect(document.activeElement?.textContent).toBe('Call');
	});

	it('keeps Tab inside the dialog', () => {
		const node = dialog('<button>First</button><button>Last</button>');
		modal(node);
		const [first, last] = node.querySelectorAll('button');

		last.focus();
		expect(tab(node)).toBe(true);
		expect(document.activeElement).toBe(first);

		expect(tab(node, true)).toBe(true);
		expect(document.activeElement).toBe(last);
	});

	it('moves focus back in when the focused control is replaced', async () => {
		const node = dialog('<button>Cancel</button>');
		modal(node);
		node.innerHTML = '<a href="tel:10111">Call police</a>';
		await tick();
		expect(document.activeElement?.textContent).toBe('Call police');
	});

	it('gives focus back when it closes', () => {
		const opener = document.createElement('button');
		document.body.append(opener);
		opener.focus();

		const node = dialog('<button>Close</button>');
		const { destroy } = modal(node);
		expect(document.activeElement?.textContent).toBe('Close');
		destroy();
		expect(document.activeElement).toBe(opener);
	});
});
