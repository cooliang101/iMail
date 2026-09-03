import { createPortal, useCallback, useEffect, useId, useRef, useState, type CSSProperties, type KeyboardEvent } from 'preact/compat';
import { CaretDown, Check } from '../icons';

export type AppSelectOption = { value: string; label: string; disabled?: boolean };

type AppSelectProps = {
  name?: string;
  value?: string;
  defaultValue?: string;
  options: AppSelectOption[];
  onValueChange?: (value: string) => void;
  className?: string;
  listboxClassName?: string;
  disabled?: boolean;
  required?: boolean;
  id?: string;
  form?: string;
  title?: string;
  tabIndex?: number;
  'aria-label'?: string;
};

function nextEnabledIndex(options: AppSelectOption[], current: number, direction: 1 | -1) {
  if (options.length === 0) return -1;
  for (let offset = 1; offset <= options.length; offset += 1) {
    const index = (current + direction * offset + options.length) % options.length;
    if (!options[index].disabled) return index;
  }
  return -1;
}

export function AppSelect({ name, value, defaultValue, options, onValueChange, className, listboxClassName, disabled = false, required = false, id, form, title, tabIndex, 'aria-label': ariaLabel }: AppSelectProps) {
  const initialValue = defaultValue ?? options.find((option) => !option.disabled)?.value ?? '';
  const [internalValue, setInternalValue] = useState(initialValue);
  const [open, setOpen] = useState(false);
  const [activeIndex, setActiveIndex] = useState(-1);
  const [listboxStyle, setListboxStyle] = useState<CSSProperties>({});
  const triggerRef = useRef<HTMLButtonElement>(null);
  const listboxRef = useRef<HTMLDivElement>(null);
  const generatedId = useId();
  const triggerId = id ?? `app-select-${generatedId}`;
  const listboxId = `${triggerId}-listbox`;
  const selectedValue = value ?? internalValue;
  const selectedIndex = options.findIndex((option) => option.value === selectedValue);
  const selectedOption = options[selectedIndex] ?? options.find((option) => !option.disabled);
  const unavailable = disabled || !selectedOption;

  const positionListbox = useCallback(() => {
    const trigger = triggerRef.current;
    if (!trigger) return;
    const rect = trigger.getBoundingClientRect();
    const viewportGap = 8;
    const listGap = 6;
    const estimatedHeight = Math.min(240, Math.max(44, options.length * 36 + 10));
    const spaceBelow = window.innerHeight - rect.bottom - viewportGap;
    const spaceAbove = rect.top - viewportGap;
    const placeAbove = spaceBelow < Math.min(160, estimatedHeight) && spaceAbove > spaceBelow;
    const width = Math.min(rect.width, window.innerWidth - viewportGap * 2);
    const left = Math.min(Math.max(viewportGap, rect.left), window.innerWidth - width - viewportGap);
    const maxHeight = Math.max(44, Math.min(240, placeAbove ? spaceAbove - listGap : spaceBelow - listGap));
    setListboxStyle({
      position: 'fixed',
      left,
      top: placeAbove ? Math.max(viewportGap, rect.top - Math.min(estimatedHeight, maxHeight) - listGap) : rect.bottom + listGap,
      width,
      maxHeight,
    });
  }, [options.length]);

  function openListbox(preferredIndex = selectedIndex) {
    if (unavailable) return;
    const fallback = options.findIndex((option) => !option.disabled);
    setActiveIndex(preferredIndex >= 0 && !options[preferredIndex]?.disabled ? preferredIndex : fallback);
    positionListbox();
    setOpen(true);
  }

  function closeListbox(focusTrigger = false) {
    setOpen(false);
    if (focusTrigger) triggerRef.current?.focus();
  }

  function selectOption(option: AppSelectOption) {
    if (option.disabled) return;
    if (value === undefined) setInternalValue(option.value);
    onValueChange?.(option.value);
    closeListbox(true);
  }

  function handleTriggerKeyDown(event: KeyboardEvent<HTMLButtonElement>) {
    if (unavailable) return;
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      if (!open) openListbox(event.key === 'ArrowDown' ? selectedIndex : nextEnabledIndex(options, selectedIndex < 0 ? 0 : selectedIndex, -1));
      else setActiveIndex((current) => nextEnabledIndex(options, current, event.key === 'ArrowDown' ? 1 : -1));
    } else if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      if (open && activeIndex >= 0) selectOption(options[activeIndex]);
      else openListbox();
    } else if (event.key === 'Escape' && open) {
      event.preventDefault();
      event.stopPropagation();
      closeListbox(true);
    } else if ((event.key === 'Home' || event.key === 'End') && open) {
      event.preventDefault();
      const indexes = options.map((option, index) => option.disabled ? -1 : index).filter((index) => index >= 0);
      setActiveIndex(event.key === 'Home' ? indexes[0] ?? -1 : indexes.at(-1) ?? -1);
    } else if (event.key === 'Tab' && open) {
      closeListbox();
    }
  }

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: PointerEvent) => {
      const target = event.target as Node;
      if (!triggerRef.current?.contains(target) && !listboxRef.current?.contains(target)) closeListbox();
    };
    const reposition = () => positionListbox();
    document.addEventListener('pointerdown', onPointerDown);
    window.addEventListener('resize', reposition);
    window.addEventListener('scroll', reposition, true);
    return () => {
      document.removeEventListener('pointerdown', onPointerDown);
      window.removeEventListener('resize', reposition);
      window.removeEventListener('scroll', reposition, true);
    };
  }, [open, positionListbox]);

  return <span className="app-select-wrap">
    {name && <input type="hidden" name={name} value={selectedOption?.value ?? ''} disabled={disabled} required={required} form={form} />}
    <button
      ref={triggerRef}
      id={triggerId}
      type="button"
      className={className ? `app-select ${className}` : 'app-select'}
      disabled={unavailable}
      title={title}
      tabIndex={tabIndex}
      aria-label={ariaLabel}
      aria-haspopup="listbox"
      aria-expanded={open}
      aria-controls={open ? listboxId : undefined}
      aria-activedescendant={open && activeIndex >= 0 ? `${listboxId}-option-${activeIndex}` : undefined}
      onClick={() => open ? closeListbox() : openListbox()}
      onKeyDown={handleTriggerKeyDown}
    >
      <span>{selectedOption?.label ?? '暂无可选项'}</span>
      <CaretDown className="app-select-caret" size={15} aria-hidden="true" />
    </button>
    {open && createPortal(<div
      ref={listboxRef}
      id={listboxId}
      role="listbox"
      aria-labelledby={ariaLabel ? undefined : triggerId}
      aria-label={ariaLabel}
      className={listboxClassName ? `app-select-listbox ${listboxClassName}` : 'app-select-listbox'}
      style={listboxStyle}
    >
      {options.map((option, index) => <button
        id={`${listboxId}-option-${index}`}
        key={option.value}
        type="button"
        role="option"
        tabIndex={-1}
        aria-selected={option.value === selectedOption?.value}
        disabled={option.disabled}
        className={`${option.value === selectedOption?.value ? 'is-selected' : ''}${index === activeIndex ? ' is-active' : ''}`}
        onClick={() => selectOption(option)}
      >
        <span>{option.label}</span>
        {option.value === selectedOption?.value && <Check size={14} weight="bold" />}
      </button>)}
    </div>, document.body)}
  </span>;
}
