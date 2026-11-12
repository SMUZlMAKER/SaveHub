import { mount } from 'svelte'
import './app.css'
import { initializeDiagnostics } from './lib/diagnostics'
import App from './App.svelte'
import LaunchDialog from './lib/components/LaunchDialog.svelte'
import SaveToast from './lib/components/SaveToast.svelte'

initializeDiagnostics()

const saveToast = new URLSearchParams(window.location.search).has('save-toast')
if (saveToast) document.body.classList.add('save-toast')
const launchDialog = new URLSearchParams(window.location.search).has('launch')
if (launchDialog) document.body.classList.add('launch-dialog')

const app = mount(saveToast ? SaveToast : launchDialog ? LaunchDialog : App, {
  target: document.getElementById('app')!,
})

export default app
