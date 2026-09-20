import { FormEvent, useState } from 'react';
import { createRoot } from 'react-dom/client';
import './style.css';

const API = import.meta.env.VITE_API_URL ?? 'http://localhost:3000';

function App() {
  const [email, setEmail] = useState('');
  const [campaignName, setCampaignName] = useState('Welcome series');
  const [notice, setNotice] = useState('');

  async function addContact(event: FormEvent) {
    event.preventDefault();
    const response = await fetch(`${API}/api/contacts`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ email }) });
    setNotice(response.ok ? 'Contact subscribed and TinyFlows notified.' : 'Unable to add the contact.');
  }

  async function createCampaign(event: FormEvent) {
    event.preventDefault();
    const response = await fetch(`${API}/api/campaigns`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ name: campaignName, subject: 'A note from Chimpboard', html_body: '<p>Hello!</p><p><a href="{{unsubscribe_url}}">Unsubscribe</a></p>' }) });
    setNotice(response.ok ? 'Campaign draft created.' : 'Unable to create the campaign.');
  }

  return <main>
    <nav><strong>Chimpboard</strong><span>Campaigns</span><span>Contacts</span><span>Automations</span><a href={`${API}/api/auth/google?state=dashboard-login`}>Sign in with Google</a></nav>
    <section className="hero"><p className="eyebrow">EMAIL AUTOMATION</p><h1>Build campaigns people want to receive.</h1><p>Audience management, automation handoffs, and a respectful unsubscribe path in one control room.</p></section>
    <section className="metrics"><article><b>0</b><span>Active contacts</span></article><article><b>0</b><span>Campaigns sent</span></article><article><b>—</b><span>Open rate</span></article></section>
    <section className="forms">
      <form onSubmit={addContact}><h2>Add a contact</h2><label>Email<input value={email} type="email" onChange={event => setEmail(event.target.value)} required placeholder="ada@example.com" /></label><button>Subscribe contact</button></form>
      <form onSubmit={createCampaign}><h2>Start a campaign</h2><label>Campaign name<input value={campaignName} onChange={event => setCampaignName(event.target.value)} required /></label><button>Create draft</button></form>
    </section>
    {notice && <p className="notice">{notice}</p>}
  </main>;
}

createRoot(document.getElementById('root')!).render(<App />);
