import { parseHTML } from 'linkedom';
import { describe, expect, it } from 'vitest';
import { canSaveImageDirectly, displayedMessageFilename, imageDownloadFilename, linkCopyValue, mailBodyUrlKind, resolveMailBodyContextTarget } from './mail-body-context';

describe('mail body context target', () => {
  it('distinguishes links, images and body background', () => {
    const { document } = parseHTML('<div id="root"><a href="https://example.test/path"><span id="link">Open</span><img id="linked-image" src="https://cdn.example.test/banner.png" alt="Banner"></a><img id="image" src="https://cdn.example.test/photo.png" alt="Photo"><p id="body">Text</p></div>');
    const root = document.querySelector<HTMLElement>('#root')!;
    expect(resolveMailBodyContextTarget(root, document.querySelector('#link'), null)).toEqual({ kind: 'link', href: 'https://example.test/path' });
    expect(resolveMailBodyContextTarget(root, document.querySelector('#linked-image'), null)).toEqual({ kind: 'link', href: 'https://example.test/path', image: { src: 'https://cdn.example.test/banner.png', alt: 'Banner' } });
    expect(resolveMailBodyContextTarget(root, document.querySelector('#image'), null)).toEqual({ kind: 'image', src: 'https://cdn.example.test/photo.png', alt: 'Photo' });
    expect(resolveMailBodyContextTarget(root, document.querySelector('#body'), null)).toEqual({ kind: 'body' });
    const text = document.querySelector('#body')?.firstChild ?? null;
    const selection = { isCollapsed: false, anchorNode: text, focusNode: text, toString: () => 'Text' } as unknown as Selection;
    expect(resolveMailBodyContextTarget(root, document.querySelector('#body'), selection)).toEqual({ kind: 'selection', text: 'Text' });
  });

  it('recognizes supported protocols and extracts copy values', () => {
    expect(mailBodyUrlKind('https://example.test')).toBe('web');
    expect(mailBodyUrlKind('mailto:owner%2Balerts@example.test')).toBe('email');
    expect(mailBodyUrlKind('javascript:alert(1)')).toBeNull();
    expect(linkCopyValue('mailto:owner%2Balerts@example.test', 'email')).toBe('owner+alerts@example.test');
  });

  it('derives safe image download names', () => {
    expect(imageDownloadFilename('https://cdn.example.test/assets/photo.webp', '')).toBe('photo.webp');
    expect(imageDownloadFilename('https://cdn.example.test/assets/CON%20%2Fphoto.png', '')).toBe('_CON _photo.png');
    expect(imageDownloadFilename('data:image/png;base64,AAAA', 'Logo: primary')).toBe('Logo_ primary.png');
    expect(canSaveImageDirectly('https://mail.example.test/logo.png', 'https://mail.example.test/inbox')).toBe(true);
    expect(canSaveImageDirectly('https://tracker.example.test/logo.png', 'https://mail.example.test/inbox')).toBe(false);
    expect(canSaveImageDirectly('data:image/png;base64,AAAA', 'https://mail.example.test/inbox')).toBe(true);
    expect(displayedMessageFilename(' Quarterly: report? ')).toBe('Quarterly_ report_.txt');
    expect(displayedMessageFilename('CON')).toBe('_CON.txt');
  });
});
