/**
 * Unit tests for NewFolderDialog initial focus.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, screen, cleanup } from '@testing-library/react';
import NewFolderDialog from './NewFolderDialog';

afterEach(cleanup);

const defaultProps = {
  isOpen: true,
  parent: '',
  existingFolders: [],
  existingPaths: [],
  onClose: () => {},
  onCreateFolder: () => {},
};

describe('NewFolderDialog initial focus', () => {
  // The name input used to be focused on a 100ms timer, which could steal
  // focus back from wherever the user had already moved it (so Enter on
  // Cancel would submit — see NewFileDialog's equivalent tests).
  it('focuses the folder name input on open', () => {
    render(<NewFolderDialog {...defaultProps} />);

    expect(document.activeElement).toBe(screen.getByLabelText('Name:'));
  });

  it('does not pull focus back to the name input later', () => {
    vi.useFakeTimers();
    try {
      render(<NewFolderDialog {...defaultProps} />);
      const cancelButton = screen.getByRole('button', { name: 'Cancel' });
      cancelButton.focus();
      vi.advanceTimersByTime(500);

      expect(document.activeElement).toBe(cancelButton);
    } finally {
      vi.useRealTimers();
    }
  });
});
