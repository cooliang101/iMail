import { type JSX } from 'preact';
import { forwardRef } from 'preact/compat';
import addressBook from '@iconify-icons/ph/address-book';
import addressBookDuotone from '@iconify-icons/ph/address-book-duotone';
import archive from '@iconify-icons/ph/archive';
import arrowBendUpLeft from '@iconify-icons/ph/arrow-bend-up-left';
import arrowBendUpRight from '@iconify-icons/ph/arrow-bend-up-right';
import arrowClockwise from '@iconify-icons/ph/arrow-clockwise';
import arrowCounterClockwise from '@iconify-icons/ph/arrow-counter-clockwise';
import arrowLeft from '@iconify-icons/ph/arrow-left';
import arrowRight from '@iconify-icons/ph/arrow-right';
import arrowsLeftRight from '@iconify-icons/ph/arrows-left-right';
import bell from '@iconify-icons/ph/bell';
import bellDuotone from '@iconify-icons/ph/bell-duotone';
import bookOpen from '@iconify-icons/ph/book-open';
import briefcase from '@iconify-icons/ph/briefcase';
import buildings from '@iconify-icons/ph/buildings';
import caretDown from '@iconify-icons/ph/caret-down';
import caretUp from '@iconify-icons/ph/caret-up';
import check from '@iconify-icons/ph/check';
import checkBold from '@iconify-icons/ph/check-bold';
import checkCircle from '@iconify-icons/ph/check-circle';
import checkCircleFill from '@iconify-icons/ph/check-circle-fill';
import clipboardText from '@iconify-icons/ph/clipboard-text';
import clock from '@iconify-icons/ph/clock';
import cloud from '@iconify-icons/ph/cloud';
import cloudDuotone from '@iconify-icons/ph/cloud-duotone';
import cloudSlash from '@iconify-icons/ph/cloud-slash';
import cloudSlashDuotone from '@iconify-icons/ph/cloud-slash-duotone';
import code from '@iconify-icons/ph/code';
import copy from '@iconify-icons/ph/copy';
import copyBold from '@iconify-icons/ph/copy-bold';
import database from '@iconify-icons/ph/database';
import downloadSimple from '@iconify-icons/ph/download-simple';
import envelope from '@iconify-icons/ph/envelope';
import envelopeDuotone from '@iconify-icons/ph/envelope-duotone';
import envelopeSimple from '@iconify-icons/ph/envelope-simple';
import envelopeSimpleFill from '@iconify-icons/ph/envelope-simple-fill';
import envelopeSimpleDuotone from '@iconify-icons/ph/envelope-simple-duotone';
import eye from '@iconify-icons/ph/eye';
import file from '@iconify-icons/ph/file';
import fileDuotone from '@iconify-icons/ph/file-duotone';
import fileArchive from '@iconify-icons/ph/file-archive';
import fileLock from '@iconify-icons/ph/file-lock';
import fileLockDuotone from '@iconify-icons/ph/file-lock-duotone';
import folder from '@iconify-icons/ph/folder';
import folderOpen from '@iconify-icons/ph/folder-open';
import folderSimple from '@iconify-icons/ph/folder-simple';
import folderSimplePlus from '@iconify-icons/ph/folder-simple-plus';
import gear from '@iconify-icons/ph/gear';
import globe from '@iconify-icons/ph/globe';
import globeDuotone from '@iconify-icons/ph/globe-duotone';
import hardDrives from '@iconify-icons/ph/hard-drives';
import hardDrivesDuotone from '@iconify-icons/ph/hard-drives-duotone';
import heart from '@iconify-icons/ph/heart';
import house from '@iconify-icons/ph/house';
import image from '@iconify-icons/ph/image';
import imageSquare from '@iconify-icons/ph/image-square';
import key from '@iconify-icons/ph/key';
import keyDuotone from '@iconify-icons/ph/key-duotone';
import keyboard from '@iconify-icons/ph/keyboard';
import linkSimple from '@iconify-icons/ph/link-simple';
import listBullets from '@iconify-icons/ph/list-bullets';
import listNumbers from '@iconify-icons/ph/list-numbers';
import lockKey from '@iconify-icons/ph/lock-key';
import magnifyingGlass from '@iconify-icons/ph/magnifying-glass';
import microsoftOutlookLogo from '@iconify-icons/ph/microsoft-outlook-logo';
import microsoftOutlookLogoFill from '@iconify-icons/ph/microsoft-outlook-logo-fill';
import minus from '@iconify-icons/ph/minus';
import minusBold from '@iconify-icons/ph/minus-bold';
import palette from '@iconify-icons/ph/palette';
import paletteFill from '@iconify-icons/ph/palette-fill';
import paperclip from '@iconify-icons/ph/paperclip';
import paperPlaneTilt from '@iconify-icons/ph/paper-plane-tilt';
import pencilSimple from '@iconify-icons/ph/pencil-simple';
import pencilSimpleDuotone from '@iconify-icons/ph/pencil-simple-duotone';
import plugsConnected from '@iconify-icons/ph/plugs-connected';
import plus from '@iconify-icons/ph/plus';
import power from '@iconify-icons/ph/power';
import quotes from '@iconify-icons/ph/quotes';
import sidebarSimple from '@iconify-icons/ph/sidebar-simple';
import spinnerGap from '@iconify-icons/ph/spinner-gap';
import square from '@iconify-icons/ph/square';
import squareBold from '@iconify-icons/ph/square-bold';
import star from '@iconify-icons/ph/star';
import starFill from '@iconify-icons/ph/star-fill';
import tag from '@iconify-icons/ph/tag';
import terminal from '@iconify-icons/ph/terminal';
import textAlignCenter from '@iconify-icons/ph/text-align-center';
import textAlignLeft from '@iconify-icons/ph/text-align-left';
import textB from '@iconify-icons/ph/text-b';
import textHTwo from '@iconify-icons/ph/text-h-two';
import textItalic from '@iconify-icons/ph/text-italic';
import textStrikethrough from '@iconify-icons/ph/text-strikethrough';
import textUnderline from '@iconify-icons/ph/text-underline';
import trash from '@iconify-icons/ph/trash';
import trashDuotone from '@iconify-icons/ph/trash-duotone';
import tray from '@iconify-icons/ph/tray';
import trayDuotone from '@iconify-icons/ph/tray-duotone';
import userCircle from '@iconify-icons/ph/user-circle';
import userCircleDuotone from '@iconify-icons/ph/user-circle-duotone';
import userPlus from '@iconify-icons/ph/user-plus';
import usersThree from '@iconify-icons/ph/users-three';
import warningCircle from '@iconify-icons/ph/warning-circle';
import warningCircleDuotone from '@iconify-icons/ph/warning-circle-duotone';
import x from '@iconify-icons/ph/x';
import xBold from '@iconify-icons/ph/x-bold';

type IconData = { body: string; width?: number; height?: number };
type AvailableWeights = { regular: IconData } & Partial<Record<Exclude<IconWeight, 'regular'>, IconData>>;

export type IconWeight = 'thin' | 'light' | 'regular' | 'bold' | 'fill' | 'duotone';
export type IconProps = Omit<JSX.SVGAttributes<SVGSVGElement>, 'size'> & {
  alt?: string;
  className?: string;
  color?: string;
  mirrored?: boolean;
  size?: string | number;
  weight?: IconWeight;
};
export type Icon = ReturnType<typeof createIcon>;

function createIcon(name: string, weights: AvailableWeights) {
  const IconComponent = forwardRef<SVGSVGElement, IconProps>(({ alt, color = 'currentColor', mirrored = false, size = '1em', weight = 'regular', style, ...svgProps }, ref) => {
    const data = weights[weight] ?? weights.regular;
    const iconStyle = {
      ...(typeof style === 'object' ? style : {}),
      color,
      ...(mirrored ? { transform: 'scaleX(-1)' } : {}),
    } as JSX.CSSProperties;
    return <svg
      ref={ref}
      xmlns="http://www.w3.org/2000/svg"
      width={size}
      height={size}
      viewBox={`0 0 ${data.width ?? 256} ${data.height ?? 256}`}
      aria-label={alt}
      aria-hidden={alt || svgProps['aria-label'] ? undefined : true}
      focusable="false"
      style={iconStyle}
      {...svgProps}
      dangerouslySetInnerHTML={{ __html: data.body }}
    />;
  });
  IconComponent.displayName = name;
  return IconComponent;
}

export const AddressBook = createIcon('AddressBook', { regular: addressBook, duotone: addressBookDuotone });
export const Archive = createIcon('Archive', { regular: archive });
export const ArrowBendUpLeft = createIcon('ArrowBendUpLeft', { regular: arrowBendUpLeft });
export const ArrowBendUpRight = createIcon('ArrowBendUpRight', { regular: arrowBendUpRight });
export const ArrowClockwise = createIcon('ArrowClockwise', { regular: arrowClockwise });
export const ArrowCounterClockwise = createIcon('ArrowCounterClockwise', { regular: arrowCounterClockwise });
export const ArrowLeft = createIcon('ArrowLeft', { regular: arrowLeft });
export const ArrowRight = createIcon('ArrowRight', { regular: arrowRight });
export const ArrowsLeftRight = createIcon('ArrowsLeftRight', { regular: arrowsLeftRight });
export const Bell = createIcon('Bell', { regular: bell, duotone: bellDuotone });
export const BookOpen = createIcon('BookOpen', { regular: bookOpen });
export const Briefcase = createIcon('Briefcase', { regular: briefcase });
export const Buildings = createIcon('Buildings', { regular: buildings });
export const CaretDown = createIcon('CaretDown', { regular: caretDown });
export const CaretUp = createIcon('CaretUp', { regular: caretUp });
export const Check = createIcon('Check', { regular: check, bold: checkBold });
export const CheckCircle = createIcon('CheckCircle', { regular: checkCircle, fill: checkCircleFill });
export const ClipboardText = createIcon('ClipboardText', { regular: clipboardText });
export const Clock = createIcon('Clock', { regular: clock });
export const Cloud = createIcon('Cloud', { regular: cloud, duotone: cloudDuotone });
export const CloudSlash = createIcon('CloudSlash', { regular: cloudSlash, duotone: cloudSlashDuotone });
export const Code = createIcon('Code', { regular: code });
export const Copy = createIcon('Copy', { regular: copy, bold: copyBold });
export const Database = createIcon('Database', { regular: database });
export const DownloadSimple = createIcon('DownloadSimple', { regular: downloadSimple });
export const Envelope = createIcon('Envelope', { regular: envelope, duotone: envelopeDuotone });
export const EnvelopeSimple = createIcon('EnvelopeSimple', { regular: envelopeSimple, fill: envelopeSimpleFill, duotone: envelopeSimpleDuotone });
export const Eye = createIcon('Eye', { regular: eye });
export const File = createIcon('File', { regular: file, duotone: fileDuotone });
export const FileArchive = createIcon('FileArchive', { regular: fileArchive });
export const FileLock = createIcon('FileLock', { regular: fileLock, duotone: fileLockDuotone });
export const Folder = createIcon('Folder', { regular: folder });
export const FolderOpen = createIcon('FolderOpen', { regular: folderOpen });
export const FolderSimple = createIcon('FolderSimple', { regular: folderSimple });
export const FolderSimplePlus = createIcon('FolderSimplePlus', { regular: folderSimplePlus });
export const Gear = createIcon('Gear', { regular: gear });
export const Globe = createIcon('Globe', { regular: globe, duotone: globeDuotone });
export const HardDrives = createIcon('HardDrives', { regular: hardDrives, duotone: hardDrivesDuotone });
export const Heart = createIcon('Heart', { regular: heart });
export const House = createIcon('House', { regular: house });
export const Image = createIcon('Image', { regular: image });
export const ImageSquare = createIcon('ImageSquare', { regular: imageSquare });
export const Key = createIcon('Key', { regular: key, duotone: keyDuotone });
export const Keyboard = createIcon('Keyboard', { regular: keyboard });
export const LinkSimple = createIcon('LinkSimple', { regular: linkSimple });
export const ListBullets = createIcon('ListBullets', { regular: listBullets });
export const ListNumbers = createIcon('ListNumbers', { regular: listNumbers });
export const LockKey = createIcon('LockKey', { regular: lockKey });
export const MagnifyingGlass = createIcon('MagnifyingGlass', { regular: magnifyingGlass });
export const MicrosoftOutlookLogo = createIcon('MicrosoftOutlookLogo', { regular: microsoftOutlookLogo, fill: microsoftOutlookLogoFill });
export const Minus = createIcon('Minus', { regular: minus, bold: minusBold });
export const Palette = createIcon('Palette', { regular: palette, fill: paletteFill });
export const Paperclip = createIcon('Paperclip', { regular: paperclip });
export const PaperPlaneTilt = createIcon('PaperPlaneTilt', { regular: paperPlaneTilt });
export const PencilSimple = createIcon('PencilSimple', { regular: pencilSimple, duotone: pencilSimpleDuotone });
export const PlugsConnected = createIcon('PlugsConnected', { regular: plugsConnected });
export const Plus = createIcon('Plus', { regular: plus });
export const Power = createIcon('Power', { regular: power });
export const Quotes = createIcon('Quotes', { regular: quotes });
export const SidebarSimple = createIcon('SidebarSimple', { regular: sidebarSimple });
export const SpinnerGap = createIcon('SpinnerGap', { regular: spinnerGap });
export const Square = createIcon('Square', { regular: square, bold: squareBold });
export const Star = createIcon('Star', { regular: star, fill: starFill });
export const Tag = createIcon('Tag', { regular: tag });
export const Terminal = createIcon('Terminal', { regular: terminal });
export const TextAlignCenter = createIcon('TextAlignCenter', { regular: textAlignCenter });
export const TextAlignLeft = createIcon('TextAlignLeft', { regular: textAlignLeft });
export const TextB = createIcon('TextB', { regular: textB });
export const TextHTwo = createIcon('TextHTwo', { regular: textHTwo });
export const TextItalic = createIcon('TextItalic', { regular: textItalic });
export const TextStrikethrough = createIcon('TextStrikethrough', { regular: textStrikethrough });
export const TextUnderline = createIcon('TextUnderline', { regular: textUnderline });
export const Trash = createIcon('Trash', { regular: trash, duotone: trashDuotone });
export const Tray = createIcon('Tray', { regular: tray, duotone: trayDuotone });
export const UserCircle = createIcon('UserCircle', { regular: userCircle, duotone: userCircleDuotone });
export const UserPlus = createIcon('UserPlus', { regular: userPlus });
export const UsersThree = createIcon('UsersThree', { regular: usersThree });
export const WarningCircle = createIcon('WarningCircle', { regular: warningCircle, duotone: warningCircleDuotone });
export const X = createIcon('X', { regular: x, bold: xBold });
