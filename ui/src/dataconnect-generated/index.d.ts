import { ConnectorConfig, DataConnect, QueryRef, QueryPromise, ExecuteQueryOptions, MutationRef, MutationPromise, DataConnectSettings } from 'firebase/data-connect';

export const connectorConfig: ConnectorConfig;
export const dataConnectSettings: DataConnectSettings;

export type TimestampString = string;
export type UUIDString = string;
export type Int64String = string;
export type DateString = string;




export interface CelestialBody_Key {
  id: UUIDString;
  __typename?: 'CelestialBody_Key';
}

export interface CreateCelestialBodyData {
  celestialBody_insert: CelestialBody_Key;
}

export interface CreateEventData {
  event_insert: Event_Key;
}

export interface CreateObservationData {
  observation_insert: Observation_Key;
}

export interface CreatePhotoSettingData {
  photoSetting_insert: PhotoSetting_Key;
}

export interface CreateSessionData {
  session_insert: Session_Key;
}

export interface CreateUserData {
  user_insert: User_Key;
}

export interface DeleteCelestialBodyData {
  celestialBody_delete?: CelestialBody_Key | null;
}

export interface DeleteEventData {
  event_delete?: Event_Key | null;
}

export interface DeleteObservationData {
  observation_delete?: Observation_Key | null;
}

export interface DeletePhotoSettingData {
  photoSetting_delete?: PhotoSetting_Key | null;
}

export interface DeleteSessionData {
  session_delete?: Session_Key | null;
}

export interface DeleteUserData {
  user_delete?: User_Key | null;
}

export interface Event_Key {
  id: UUIDString;
  __typename?: 'Event_Key';
}

export interface GetCelestialBodyData {
  celestialBody?: {
    commonName: string;
    scientificName: string;
  };
}

export interface GetEventData {
  event?: {
    eventName: string;
    description?: string | null;
  };
}

export interface GetObservationData {
  observation?: {
    notes?: string | null;
    celestialBody: {
      commonName: string;
    };
  };
}

export interface GetPhotoSettingData {
  photoSetting?: {
    cameraModel: string;
    iso: number;
  };
}

export interface GetSessionData {
  session?: {
    sessionName: string;
    user: {
      email: string;
    };
  };
}

export interface GetUserData {
  user?: {
    email: string;
    displayName: string;
  };
}

export interface ListCelestialBodiesData {
  celestialBodies: ({
    commonName: string;
    objectType: string;
  })[];
}

export interface ListEventsData {
  events: ({
    eventName: string;
    startTime: TimestampString;
  })[];
}

export interface ListMyObservationsData {
  observations: ({
    observationTimestamp: TimestampString;
    notes?: string | null;
  })[];
}

export interface ListMyPhotoSettingsData {
  photoSettings: ({
    cameraModel: string;
    exposureSeconds: number;
  })[];
}

export interface ListMySessionsData {
  sessions: ({
    sessionName: string;
    startDate?: DateString | null;
  })[];
}

export interface ListUsersData {
  users: ({
    id: UUIDString;
    displayName: string;
  } & User_Key)[];
}

export interface Observation_Key {
  id: UUIDString;
  __typename?: 'Observation_Key';
}

export interface PhotoSetting_Key {
  id: UUIDString;
  __typename?: 'PhotoSetting_Key';
}

export interface Session_Key {
  id: UUIDString;
  __typename?: 'Session_Key';
}

export interface UpdateCelestialBodyData {
  celestialBody_update?: CelestialBody_Key | null;
}

export interface UpdateEventData {
  event_update?: Event_Key | null;
}

export interface UpdateObservationData {
  observation_update?: Observation_Key | null;
}

export interface UpdatePhotoSettingData {
  photoSetting_update?: PhotoSetting_Key | null;
}

export interface UpdateSessionData {
  session_update?: Session_Key | null;
}

export interface UpdateUserData {
  user_update?: User_Key | null;
}

export interface User_Key {
  id: UUIDString;
  __typename?: 'User_Key';
}

interface CreateUserRef {
  /* Allow users to create refs without passing in DataConnect */
  (): MutationRef<CreateUserData, undefined>;
  /* Allow users to pass in custom DataConnect instances */
  (dc: DataConnect): MutationRef<CreateUserData, undefined>;
  operationName: string;
}
export const createUserRef: CreateUserRef;

export function createUser(): MutationPromise<CreateUserData, undefined>;
export function createUser(dc: DataConnect): MutationPromise<CreateUserData, undefined>;

interface UpdateUserRef {
  /* Allow users to create refs without passing in DataConnect */
  (): MutationRef<UpdateUserData, undefined>;
  /* Allow users to pass in custom DataConnect instances */
  (dc: DataConnect): MutationRef<UpdateUserData, undefined>;
  operationName: string;
}
export const updateUserRef: UpdateUserRef;

export function updateUser(): MutationPromise<UpdateUserData, undefined>;
export function updateUser(dc: DataConnect): MutationPromise<UpdateUserData, undefined>;

interface DeleteUserRef {
  /* Allow users to create refs without passing in DataConnect */
  (): MutationRef<DeleteUserData, undefined>;
  /* Allow users to pass in custom DataConnect instances */
  (dc: DataConnect): MutationRef<DeleteUserData, undefined>;
  operationName: string;
}
export const deleteUserRef: DeleteUserRef;

export function deleteUser(): MutationPromise<DeleteUserData, undefined>;
export function deleteUser(dc: DataConnect): MutationPromise<DeleteUserData, undefined>;

interface GetUserRef {
  /* Allow users to create refs without passing in DataConnect */
  (): QueryRef<GetUserData, undefined>;
  /* Allow users to pass in custom DataConnect instances */
  (dc: DataConnect): QueryRef<GetUserData, undefined>;
  operationName: string;
}
export const getUserRef: GetUserRef;

export function getUser(options?: ExecuteQueryOptions): QueryPromise<GetUserData, undefined>;
export function getUser(dc: DataConnect, options?: ExecuteQueryOptions): QueryPromise<GetUserData, undefined>;

interface ListUsersRef {
  /* Allow users to create refs without passing in DataConnect */
  (): QueryRef<ListUsersData, undefined>;
  /* Allow users to pass in custom DataConnect instances */
  (dc: DataConnect): QueryRef<ListUsersData, undefined>;
  operationName: string;
}
export const listUsersRef: ListUsersRef;

export function listUsers(options?: ExecuteQueryOptions): QueryPromise<ListUsersData, undefined>;
export function listUsers(dc: DataConnect, options?: ExecuteQueryOptions): QueryPromise<ListUsersData, undefined>;

interface CreateCelestialBodyRef {
  /* Allow users to create refs without passing in DataConnect */
  (): MutationRef<CreateCelestialBodyData, undefined>;
  /* Allow users to pass in custom DataConnect instances */
  (dc: DataConnect): MutationRef<CreateCelestialBodyData, undefined>;
  operationName: string;
}
export const createCelestialBodyRef: CreateCelestialBodyRef;

export function createCelestialBody(): MutationPromise<CreateCelestialBodyData, undefined>;
export function createCelestialBody(dc: DataConnect): MutationPromise<CreateCelestialBodyData, undefined>;

interface UpdateCelestialBodyRef {
  /* Allow users to create refs without passing in DataConnect */
  (): MutationRef<UpdateCelestialBodyData, undefined>;
  /* Allow users to pass in custom DataConnect instances */
  (dc: DataConnect): MutationRef<UpdateCelestialBodyData, undefined>;
  operationName: string;
}
export const updateCelestialBodyRef: UpdateCelestialBodyRef;

export function updateCelestialBody(): MutationPromise<UpdateCelestialBodyData, undefined>;
export function updateCelestialBody(dc: DataConnect): MutationPromise<UpdateCelestialBodyData, undefined>;

interface DeleteCelestialBodyRef {
  /* Allow users to create refs without passing in DataConnect */
  (): MutationRef<DeleteCelestialBodyData, undefined>;
  /* Allow users to pass in custom DataConnect instances */
  (dc: DataConnect): MutationRef<DeleteCelestialBodyData, undefined>;
  operationName: string;
}
export const deleteCelestialBodyRef: DeleteCelestialBodyRef;

export function deleteCelestialBody(): MutationPromise<DeleteCelestialBodyData, undefined>;
export function deleteCelestialBody(dc: DataConnect): MutationPromise<DeleteCelestialBodyData, undefined>;

interface GetCelestialBodyRef {
  /* Allow users to create refs without passing in DataConnect */
  (): QueryRef<GetCelestialBodyData, undefined>;
  /* Allow users to pass in custom DataConnect instances */
  (dc: DataConnect): QueryRef<GetCelestialBodyData, undefined>;
  operationName: string;
}
export const getCelestialBodyRef: GetCelestialBodyRef;

export function getCelestialBody(options?: ExecuteQueryOptions): QueryPromise<GetCelestialBodyData, undefined>;
export function getCelestialBody(dc: DataConnect, options?: ExecuteQueryOptions): QueryPromise<GetCelestialBodyData, undefined>;

interface ListCelestialBodiesRef {
  /* Allow users to create refs without passing in DataConnect */
  (): QueryRef<ListCelestialBodiesData, undefined>;
  /* Allow users to pass in custom DataConnect instances */
  (dc: DataConnect): QueryRef<ListCelestialBodiesData, undefined>;
  operationName: string;
}
export const listCelestialBodiesRef: ListCelestialBodiesRef;

export function listCelestialBodies(options?: ExecuteQueryOptions): QueryPromise<ListCelestialBodiesData, undefined>;
export function listCelestialBodies(dc: DataConnect, options?: ExecuteQueryOptions): QueryPromise<ListCelestialBodiesData, undefined>;

interface CreateEventRef {
  /* Allow users to create refs without passing in DataConnect */
  (): MutationRef<CreateEventData, undefined>;
  /* Allow users to pass in custom DataConnect instances */
  (dc: DataConnect): MutationRef<CreateEventData, undefined>;
  operationName: string;
}
export const createEventRef: CreateEventRef;

export function createEvent(): MutationPromise<CreateEventData, undefined>;
export function createEvent(dc: DataConnect): MutationPromise<CreateEventData, undefined>;

interface UpdateEventRef {
  /* Allow users to create refs without passing in DataConnect */
  (): MutationRef<UpdateEventData, undefined>;
  /* Allow users to pass in custom DataConnect instances */
  (dc: DataConnect): MutationRef<UpdateEventData, undefined>;
  operationName: string;
}
export const updateEventRef: UpdateEventRef;

export function updateEvent(): MutationPromise<UpdateEventData, undefined>;
export function updateEvent(dc: DataConnect): MutationPromise<UpdateEventData, undefined>;

interface DeleteEventRef {
  /* Allow users to create refs without passing in DataConnect */
  (): MutationRef<DeleteEventData, undefined>;
  /* Allow users to pass in custom DataConnect instances */
  (dc: DataConnect): MutationRef<DeleteEventData, undefined>;
  operationName: string;
}
export const deleteEventRef: DeleteEventRef;

export function deleteEvent(): MutationPromise<DeleteEventData, undefined>;
export function deleteEvent(dc: DataConnect): MutationPromise<DeleteEventData, undefined>;

interface GetEventRef {
  /* Allow users to create refs without passing in DataConnect */
  (): QueryRef<GetEventData, undefined>;
  /* Allow users to pass in custom DataConnect instances */
  (dc: DataConnect): QueryRef<GetEventData, undefined>;
  operationName: string;
}
export const getEventRef: GetEventRef;

export function getEvent(options?: ExecuteQueryOptions): QueryPromise<GetEventData, undefined>;
export function getEvent(dc: DataConnect, options?: ExecuteQueryOptions): QueryPromise<GetEventData, undefined>;

interface ListEventsRef {
  /* Allow users to create refs without passing in DataConnect */
  (): QueryRef<ListEventsData, undefined>;
  /* Allow users to pass in custom DataConnect instances */
  (dc: DataConnect): QueryRef<ListEventsData, undefined>;
  operationName: string;
}
export const listEventsRef: ListEventsRef;

export function listEvents(options?: ExecuteQueryOptions): QueryPromise<ListEventsData, undefined>;
export function listEvents(dc: DataConnect, options?: ExecuteQueryOptions): QueryPromise<ListEventsData, undefined>;

interface CreateSessionRef {
  /* Allow users to create refs without passing in DataConnect */
  (): MutationRef<CreateSessionData, undefined>;
  /* Allow users to pass in custom DataConnect instances */
  (dc: DataConnect): MutationRef<CreateSessionData, undefined>;
  operationName: string;
}
export const createSessionRef: CreateSessionRef;

export function createSession(): MutationPromise<CreateSessionData, undefined>;
export function createSession(dc: DataConnect): MutationPromise<CreateSessionData, undefined>;

interface UpdateSessionRef {
  /* Allow users to create refs without passing in DataConnect */
  (): MutationRef<UpdateSessionData, undefined>;
  /* Allow users to pass in custom DataConnect instances */
  (dc: DataConnect): MutationRef<UpdateSessionData, undefined>;
  operationName: string;
}
export const updateSessionRef: UpdateSessionRef;

export function updateSession(): MutationPromise<UpdateSessionData, undefined>;
export function updateSession(dc: DataConnect): MutationPromise<UpdateSessionData, undefined>;

interface DeleteSessionRef {
  /* Allow users to create refs without passing in DataConnect */
  (): MutationRef<DeleteSessionData, undefined>;
  /* Allow users to pass in custom DataConnect instances */
  (dc: DataConnect): MutationRef<DeleteSessionData, undefined>;
  operationName: string;
}
export const deleteSessionRef: DeleteSessionRef;

export function deleteSession(): MutationPromise<DeleteSessionData, undefined>;
export function deleteSession(dc: DataConnect): MutationPromise<DeleteSessionData, undefined>;

interface GetSessionRef {
  /* Allow users to create refs without passing in DataConnect */
  (): QueryRef<GetSessionData, undefined>;
  /* Allow users to pass in custom DataConnect instances */
  (dc: DataConnect): QueryRef<GetSessionData, undefined>;
  operationName: string;
}
export const getSessionRef: GetSessionRef;

export function getSession(options?: ExecuteQueryOptions): QueryPromise<GetSessionData, undefined>;
export function getSession(dc: DataConnect, options?: ExecuteQueryOptions): QueryPromise<GetSessionData, undefined>;

interface ListMySessionsRef {
  /* Allow users to create refs without passing in DataConnect */
  (): QueryRef<ListMySessionsData, undefined>;
  /* Allow users to pass in custom DataConnect instances */
  (dc: DataConnect): QueryRef<ListMySessionsData, undefined>;
  operationName: string;
}
export const listMySessionsRef: ListMySessionsRef;

export function listMySessions(options?: ExecuteQueryOptions): QueryPromise<ListMySessionsData, undefined>;
export function listMySessions(dc: DataConnect, options?: ExecuteQueryOptions): QueryPromise<ListMySessionsData, undefined>;

interface CreateObservationRef {
  /* Allow users to create refs without passing in DataConnect */
  (): MutationRef<CreateObservationData, undefined>;
  /* Allow users to pass in custom DataConnect instances */
  (dc: DataConnect): MutationRef<CreateObservationData, undefined>;
  operationName: string;
}
export const createObservationRef: CreateObservationRef;

export function createObservation(): MutationPromise<CreateObservationData, undefined>;
export function createObservation(dc: DataConnect): MutationPromise<CreateObservationData, undefined>;

interface UpdateObservationRef {
  /* Allow users to create refs without passing in DataConnect */
  (): MutationRef<UpdateObservationData, undefined>;
  /* Allow users to pass in custom DataConnect instances */
  (dc: DataConnect): MutationRef<UpdateObservationData, undefined>;
  operationName: string;
}
export const updateObservationRef: UpdateObservationRef;

export function updateObservation(): MutationPromise<UpdateObservationData, undefined>;
export function updateObservation(dc: DataConnect): MutationPromise<UpdateObservationData, undefined>;

interface DeleteObservationRef {
  /* Allow users to create refs without passing in DataConnect */
  (): MutationRef<DeleteObservationData, undefined>;
  /* Allow users to pass in custom DataConnect instances */
  (dc: DataConnect): MutationRef<DeleteObservationData, undefined>;
  operationName: string;
}
export const deleteObservationRef: DeleteObservationRef;

export function deleteObservation(): MutationPromise<DeleteObservationData, undefined>;
export function deleteObservation(dc: DataConnect): MutationPromise<DeleteObservationData, undefined>;

interface GetObservationRef {
  /* Allow users to create refs without passing in DataConnect */
  (): QueryRef<GetObservationData, undefined>;
  /* Allow users to pass in custom DataConnect instances */
  (dc: DataConnect): QueryRef<GetObservationData, undefined>;
  operationName: string;
}
export const getObservationRef: GetObservationRef;

export function getObservation(options?: ExecuteQueryOptions): QueryPromise<GetObservationData, undefined>;
export function getObservation(dc: DataConnect, options?: ExecuteQueryOptions): QueryPromise<GetObservationData, undefined>;

interface ListMyObservationsRef {
  /* Allow users to create refs without passing in DataConnect */
  (): QueryRef<ListMyObservationsData, undefined>;
  /* Allow users to pass in custom DataConnect instances */
  (dc: DataConnect): QueryRef<ListMyObservationsData, undefined>;
  operationName: string;
}
export const listMyObservationsRef: ListMyObservationsRef;

export function listMyObservations(options?: ExecuteQueryOptions): QueryPromise<ListMyObservationsData, undefined>;
export function listMyObservations(dc: DataConnect, options?: ExecuteQueryOptions): QueryPromise<ListMyObservationsData, undefined>;

interface CreatePhotoSettingRef {
  /* Allow users to create refs without passing in DataConnect */
  (): MutationRef<CreatePhotoSettingData, undefined>;
  /* Allow users to pass in custom DataConnect instances */
  (dc: DataConnect): MutationRef<CreatePhotoSettingData, undefined>;
  operationName: string;
}
export const createPhotoSettingRef: CreatePhotoSettingRef;

export function createPhotoSetting(): MutationPromise<CreatePhotoSettingData, undefined>;
export function createPhotoSetting(dc: DataConnect): MutationPromise<CreatePhotoSettingData, undefined>;

interface UpdatePhotoSettingRef {
  /* Allow users to create refs without passing in DataConnect */
  (): MutationRef<UpdatePhotoSettingData, undefined>;
  /* Allow users to pass in custom DataConnect instances */
  (dc: DataConnect): MutationRef<UpdatePhotoSettingData, undefined>;
  operationName: string;
}
export const updatePhotoSettingRef: UpdatePhotoSettingRef;

export function updatePhotoSetting(): MutationPromise<UpdatePhotoSettingData, undefined>;
export function updatePhotoSetting(dc: DataConnect): MutationPromise<UpdatePhotoSettingData, undefined>;

interface DeletePhotoSettingRef {
  /* Allow users to create refs without passing in DataConnect */
  (): MutationRef<DeletePhotoSettingData, undefined>;
  /* Allow users to pass in custom DataConnect instances */
  (dc: DataConnect): MutationRef<DeletePhotoSettingData, undefined>;
  operationName: string;
}
export const deletePhotoSettingRef: DeletePhotoSettingRef;

export function deletePhotoSetting(): MutationPromise<DeletePhotoSettingData, undefined>;
export function deletePhotoSetting(dc: DataConnect): MutationPromise<DeletePhotoSettingData, undefined>;

interface GetPhotoSettingRef {
  /* Allow users to create refs without passing in DataConnect */
  (): QueryRef<GetPhotoSettingData, undefined>;
  /* Allow users to pass in custom DataConnect instances */
  (dc: DataConnect): QueryRef<GetPhotoSettingData, undefined>;
  operationName: string;
}
export const getPhotoSettingRef: GetPhotoSettingRef;

export function getPhotoSetting(options?: ExecuteQueryOptions): QueryPromise<GetPhotoSettingData, undefined>;
export function getPhotoSetting(dc: DataConnect, options?: ExecuteQueryOptions): QueryPromise<GetPhotoSettingData, undefined>;

interface ListMyPhotoSettingsRef {
  /* Allow users to create refs without passing in DataConnect */
  (): QueryRef<ListMyPhotoSettingsData, undefined>;
  /* Allow users to pass in custom DataConnect instances */
  (dc: DataConnect): QueryRef<ListMyPhotoSettingsData, undefined>;
  operationName: string;
}
export const listMyPhotoSettingsRef: ListMyPhotoSettingsRef;

export function listMyPhotoSettings(options?: ExecuteQueryOptions): QueryPromise<ListMyPhotoSettingsData, undefined>;
export function listMyPhotoSettings(dc: DataConnect, options?: ExecuteQueryOptions): QueryPromise<ListMyPhotoSettingsData, undefined>;

