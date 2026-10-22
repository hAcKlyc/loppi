// Keep the test runner usable when the real component enters the native top layer.
// This only moves the fixture toolbar; production modality remains intact.
const toolbar = document.querySelector('body > aside');
if (toolbar) {
 const place = () => {
  const dialogs = document.querySelectorAll<HTMLDialogElement>('dialog[open]');
  const owner = dialogs[dialogs.length - 1] ?? document.body;
  if (toolbar.parentElement !== owner) owner.append(toolbar);
 };
 new MutationObserver(place).observe(document.body, {childList:true, subtree:true, attributes:true, attributeFilter:['open']});
 place();
}
