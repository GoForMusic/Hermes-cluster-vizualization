import { screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import { renderWithHub } from './render';
import { Rules } from '../features/admin/pages/Rules';

describe('the alert rules page', () => {
  it('saves a threshold when the person leaves the field, not on every keystroke', async () => {
    const { hub, store } = await renderWithHub(<Rules />);
    const user = userEvent.setup();
    const field = screen.getByDisplayValue('85');
    await user.clear(field);
    await user.type(field, '70');
    expect(hub.saved).toHaveLength(0);
    await user.tab();
    expect(hub.saved).toHaveLength(1);
    expect(store.getState().settings.rules.find((r) => r.id === 'volume-usage')!.value).toBe(70);
  });

  it('switches a rule off and on', async () => {
    const { store } = await renderWithHub(<Rules />);
    const user = userEvent.setup();
    await user.click(screen.getByLabelText('Host unreachable enabled'));
    expect(store.getState().settings.rules.find((r) => r.id === 'host-down')!.enabled).toBe(false);
  });
});
