import '@testing-library/jest-dom/vitest';
import { cleanup } from '@testing-library/svelte';
import { afterEach } from 'vitest';

afterEach(cleanup);
HTMLDialogElement.prototype.showModal = function () {
  this.setAttribute('open', '');
  this.querySelector<HTMLButtonElement>('button')?.focus();
};
HTMLDialogElement.prototype.close = function () {
  this.removeAttribute('open');
};
